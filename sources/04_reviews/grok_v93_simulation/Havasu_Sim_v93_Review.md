# Havasu Home Quotes: v9.3 simulation review

Every number here comes from a **simulation**: no real person, business, text, call or payment was involved, and no real service was contacted. Labels: **[G]** = Given (stated in the brief), **[A]** = Assumed (a behaviour rate chosen by me or by the brief), **[M]** = Measured (sim output; mean over seeds 1–50 unless stated). A Measured number is only as good as the Assumed rates that drive it.

## Verdict

**Not ready to go live, and it should not go live before the legal gate.** The unit economics hold up [M, sim]: in the Normal scenario (5 requests/day [G], 60 days) profit is $1,943 per 60 days (p10–p90 $960–$2,967) [M, sim], and cash never drops below $726 [M, sim]. The app, though, breaks its own promises in ways an outside observer would notice. Homeowner names and addresses reach the AI and the job cards. A fake business can be switched on and handed a homeowner's details. A homeowner who says "cancel" or "never mind" can still end up with a business charged and holding their address. Masked calls sometimes ring the wrong homeowner. Some intake conversations stall forever. A Twilio suspension has no fallback. The **$20k–$50k a year** goal looks reachable at about 4.7 and 14.3 requests/day [M, sim]. **$20k–$50k a month is not reachable in this market**: profit levels off near $12k a month once every local business is already getting every card [M, sim].

## Conflicts with your current decisions

1. **Goal: $20k–$50k a MONTH vs the brief's "a YEAR".** The brief (Parts 1 and 12) asks for $20,000 and $50,000 profit **a year**. Your current goal is $20k–$50k **a month**. I report both:
   - **Per year:** about **4.7 requests/day** for $20k and **14.3/day** for $50k [M, sim; steady-state demand sweep].
   - **Per month:** not reached at any demand level tested (up to 100/day). Monthly profit levels off at about $11,798 [M, sim]. A straight-line projection would say 435 and 1,687 requests/day, but that is meaningless: Lake Havasu City has about 57k residents [G], and the business pool saturates long before then.
   - **What a monthly goal would actually need:** more towns, higher fees, or a different product. Please confirm which goal is real; every recommendation below is sized to the yearly one.
2. **No homeowner name, phone or address may go into any AI tool, including Claude.** The plan breaks this in three places:
   - **Part 5.4 has the AI read the raw request.** People write "This is Ruth Hernandez at 2060 Cherry Tree…" in their first text. It also has the AI *scrub* the description, which by definition means the AI sees what it is scrubbing.
   - **The AI screens photos,** which can show house numbers.
   - **Part 11 only forbids this "unless zero retention is in place".** That directly conflicts with your rule, which has no such exception.
   - **Measured result:** the AI received a homeowner's name, phone or address 68.4 times per 60-day run in the Normal scenario [M, sim; check P3, every seed failed].
   - **Fix:** a plain-code scrubber runs *before* any AI call (names after "this is / my name is", street-number patterns, 7+ digits, emails), the AI only ever sees the scrubbed text, and photos are screened by code (OCR digits and faces blurred) or by Casey. With that fix (S17a), P3 dropped to 7.5 per run. The leftovers were things like "John, [removed]", so a code scrubber is still not perfect, and every scrubber miss is a rule breach.
3. **Legal and LLC work is deferred, with a hard gate before any business signs, any live charge or any real homeowner request.** These items in the plan would happen before that gate or need the entity first:
   - **A2P 10DLC brand and campaign registration** (Part 7; $19.50 one-time + $1.50/month [G]) needs a legal business name and EIN. Do it at the gate, not before.
   - **Stripe account and customer creation at sign-up** (Part 5.5) creates a payment relationship with a business, so it sits behind the gate.
   - **Business sign-ups, identity checks and mailed letters** (Parts 3 #13 and 8.2) contact real businesses, so they sit behind the gate.
   - **The city business license and terms of service** need an entity.
   - **Pre-gate work** should be limited to this simulation, code, the scrubber, test numbers and fake data.
4. **No hard calendar dates.** The brief carries dated labels (the date on the change list, a permit report month) as source references, not plan dates. This report uses only phases (pre-gate, hand-run test, launch, growth month N) and relative days (day 30 of a run).
5. **Scope changes from the approved hand-run test (your choices, noted neutrally).** The approved hand-run test was Lake Havasu City only, licensed trades only, fees of $45/$59/$149, and no automated line. v9.3 + Oct 26 adds 25 categories including unlicensed and recurring ones, $25/$75/$199 tiers, a fully automated text line, and neighbouring towns later. Risk judgement:
   - **More businesses and more matching:** the approved scope (S17b) keeps only 8 businesses vs 47 [M, sim], gets 0.93 quotes per job vs 1.34, and shows 0.58 vs 0.83 confirmed picks. So the wider scope is what makes the matching work.
   - **More exposure:** the wider scope also brings the handyman-exemption, permit, gas and pest-license matching failures (X1/X3/X4), more fake-business openings (unlicensed categories have no ROC identity chain), and more AI handling of personal data.
   - **Profit is close:** S17b profit $1,864 vs $1,943 per 60 days [M, sim; paired diff -79 ± 165].
   - **My read:** the automated line is the biggest risk increase, because every bug below exists only because software, not Casey, is making the move. A hand-run phase on the wider category list would catch most of them cheaply.
6. **Ask Hava (askhava.com)** is modelled with **zero** demand [G] and is not part of any number here.

### Licensing check (attorney must confirm before the gate)
- **Handyman exemption, A.R.S. §32-1121(A)(14):**
  - **The $1,000 limit** covers the *total* job, labour plus materials [G]. The 2025 bill text (HB2120, linked in the brief) is the source; an attorney should confirm the current limit before launch. The app refuses handyman quotes of $1,000 or more, using the top of a range, but only in the handyman and licensed categories. A handyman-type business quoting $2,408 on pressure washing was shown and charged $199 (seed 16, #1216). That is legal only if the work has no sealing or painting.
  - **The permit rule:** a job needing a permit is never exempt. The app never asks whether a permit is needed (the `permit_question` fix is off by default), so water-heater replacements, which need a permit in most cases, reached handymen (X1/X3).
  - **No splitting:** the app has no rule against repeat cards for the same address and category inside a short window, and it needs one.
  - **The "not a licensed contractor" disclosure:** the exemption requires handymen to say so in advertising and contracts. Their quote card shows no such line (the `handyman_disclosure` fix is off by default). Add it to every handyman quote card and to the business terms.
- **Fee that rises with quote size:** a $25/$75/$199 fee tied to the chosen quote looks like a *share of the contract price*, i.e. a commission, rather than a flat referral or listing fee.
  - Combined with the app *refusing* quotes, reading and reformatting quotes, and controlling the pick-and-confirm step, that strengthens an argument that the platform is acting "by or through others" under §32-1101's contractor definition, against the "we're just compiling the information" positioning.
  - Lower-risk shapes: a flat per-introduction fee (alt3, but it costs profit, see Price reaction) or a per-category flat fee.
  - Either way, an attorney should review the tier structure, the quote refusal rule, and the wording "we gather quotes; you choose" before the gate.
- **Pest/weed control:** routing "weed control" only to Dept. of Agriculture licensees held (X2: 0 failures in Normal).
- **Pool repair gas heaters and gas appliances:** they reached a handyman in a few runs (X3 below).

## 1. Could you run code?

**Yes.** A standard-library Python 3 discrete-event simulator was built in `sim_v93/`, with fake Twilio (SMS, MMS, Verify, voice), Stripe, Claude, ROC, Dept. of Agriculture and city-list sites, email and mail. Every scenario was run:
- **Part 10 scenarios S01–S15 and S17a–h:** seeds **1–50** each, 60 days each.
- **S02 Growth:** seeds 1–50, 730 days (24 months).
- **S16:** 6 fee structures × s50 0.10/0.15/0.20 plus the first-90-days variant, seeds 1–50.
- **Long runs for Part 13:** 365 days, seeds 1–20.
- **Demand sweep:** 2–100 requests/day, 120 days, seeds 1–8.

Run log: `done 2330 runs in 277 s; errors: 0 []`. Checks are judged only from outside-world records: the vendor logs (texts actually delivered, Stripe charges, connected calls, AI inputs) and each simulated person's own memory. They never use app status labels.

## 2. Summary: top 5

1. **Homeowner personal data reaches the AI and the job cards** [M, sim]. P3: 68 AI inputs per run carried a name, phone or address. P2: 20 job cards per run carried a name or contact phrase. Both failed in 50/50 runs. This breaks your no-PII-to-AI rule and the card promise.
2. **Fake or unverified businesses can receive homeowner details** [M, sim]: P1 failed in 29/50 Normal runs and 35/50 Adversarial runs. The cause: when the ROC result is "unknown" or the business is in an unlicensed category, Casey's approve-signup screen can activate it with no completed identity chain.
3. **Cancels don't reliably stop money or details** [M, sim]. M1 (charge after cancel or on a dead job): 1.52/run, 42/50 runs. H6: "please cancel" / "never mind" not honoured, 1.88/run. The app closes the wrong job when a homeowner has several, and turns "Never mind…" into a new request.
4. **The economics work for the YEAR goal only** [M, sim]: about 4.7/day for $20k/yr and 14.3/day for $50k/yr. In the growth scenario the $20k/yr pace first arrives in month 6 (median); the $50k/yr pace is not reached in the 24-month mean. Business churn is high: about 127 joined vs 48 kept over 24 months, because $25 on a ~$300 repair crosses the assumed fee-too-high line.
5. **A 30-day Twilio suspension costs $1,514 and +124 Casey minutes/week (~18 h)** per 60-day run [M, sim]. There is no fallback channel: 145 broken promises per run, and new requests go unanswered (H4).

## 3. Problems found, ranked by severity

Reproductions use `python3 trace.py "<overrides>" SEED JOB` (overrides `{}` = Normal). Days are run-relative; times are simulated clock times. Rates are failures per 60-day run [M, sim].

| # | Severity | Problem | Scenario, seed, day | What happened, step by step | Suggested fix |
|---|---|---|---|---|---|
| 1 | Critical | Homeowner name, address and phone sent to the AI (P3: 68/run, 50/50 runs) | S01, seed 1, day 2 11:51, job #1056 | 1) The homeowner's first text: "boat outboard motor won't start This is Ruth Hernandez at 2060 Cherry Tree Blvd, call me 206-500-0015." 2) The app sends the raw text to the AI to read the request (Part 5.4). 3) The fake Claude input log contains the name, the address and the phone. | Plain-code scrubber before *every* AI call ("this is / my name is" + capitalised words, street-number patterns, 7+ digits, emails). The AI only sees scrubbed text. Photos are screened by code (OCR for digits, face blur) or by Casey. Remove the Part 11 "unless zero retention" exception. |
| 2 | Critical | Names and contact phrases on job cards (P2: 20/run, 50/50) | S01, seed 1, job #1056, card to B0247 | 1) The same text is scrubbed: the full name, address and phone are removed. 2) "(ask for Ruth)" survives the AI scrub and the code pass. 3) The delivered card reads "…boat outboard motor won't start (ask for Ruth)". | Code pass: drop any sentence containing "ask for", "call me", "I'm", "this is" or a capitalised given name from a name list. Keep the weekly 10-card spot-check. |
| 3 | Critical | Fake or unverified businesses activated and given details (P1: 29/50 Normal runs, 3.5/run Adversarial; B3) | S01, seed 1, business F0380 → job #1221, day 36 17:57 | 1) F0380 (fake) signs up in an unlicensed category. 2) The only identity step is a code sent to *the phone F0380 itself typed in*, which it passes, then a manual approval by Casey. 3) It quotes and is picked on job #1221. 4) It confirms and pays, and gets the homeowner's first name and address. Related case, seed 1 B0019: the ROC result was 'unknown', the sign-up was held, Casey's approval activated it with no identity code at all, and the licence was never read back (B2). | Never activate on approval alone. Licensed: require a code to the phone or email on the ROC record, falling through voice and letter. Unlicensed: require a city-list name match plus a mailed letter code, or an insurance certificate. S17e (insurance required) cut active fakes from .36 to .20 per run. |
| 4 | High | Charge and details after the homeowner cancelled (M1 1.52/run, 42/50; M6; H6 1.88/run) | S01, seed 1, homeowner +19285000063, job #1126, days 14–17 | 1) The homeowner has two requests open (#1111 yard, #1112 doors). 2) Day 15 20:15 "please cancel, I found someone": the app closes only one job. 3) Day 16 15:47 "Never mind, neighbor fixed it" is not treated as a cancel (the never-mind rule is off by default). Its words match no live job, so the app opens a *new* job #1126, carrying the door text, and sends cards to B0303 and B0304. 4) Day 17 08:45 a pick is recorded. 5) Day 17 09:52 B0303 pays $25 and receives "James, James, 1082 Kiowa Blvd". The money is kept (M6). | When there is more than one open job, ask "Cancel which? 1) yard 2) doors 3) both". Treat never-mind / no-longer-need / ya-no as cancel intent. Never open a new job from a cancel-like message. Refund anything charged after a cancel. |
| 5 | High | Masked calls ring the wrong homeowner; the PIN is ignored (C1 7.1/run; C2 4.1/run) | S01, seed 1, job #1092, day 9 09:53 to day 10 17:14 | 1) Sun Hvac 34 (B0034) has two connected jobs. 2) It calls the masked line four times (one call with PIN 7548). 3) With more than one job the router picks the *most recently released* job and ignores the PIN, so every call rings +13125000045, another homeowner. 4) Day 12: Ruth is told "Sun Hvac 34 didn't call". The business had paid $25. | With more than one job, always ask for the PIN and route only by PIN (the `pin_when_multi_job` fix). After 2 bad PINs, read out the first name and street before connecting. Or use one proxy number per job. |
| 6 | High | Twilio suspension has no fallback (S07: H4 140/run, N1 125/run, 145 broken promises/run) | S07, any seed, days 30–60 | 1) Day 30: the number is suspended and every SMS fails. 2) New requests get no reply, picks can't be made by text, and businesses miss picks. 3) Jobs pile up in the 'Needs you' list; Casey's time rises by about 124 min/week. | A backup number on a second provider, registered at the gate. Email and web-chat fallback for anyone with an email or a live web session. A status line on the website. Strict 10DLC compliance (the T1/T2 fixes below) to avoid suspension in the first place. |
| 7 | High | Intake jobs stall forever (N1 26/run, 50/50) | S01, seed 1, job #1086, from day 8 08:02 | 1) The app asks "what's the street address and your first name?" 2) The homeowner never answers. 3) There is no reminder and no close; the job is still in Intake at day 60. | Reminder at 24 h, close at 72 h ("Text us any time to restart"). Use the same timeout pattern for every waiting state. |
| 8 | High | Unlicensed handyman gets permit or gas work (X1 12/run, 49/50; X3 0.38/run) | S01, seed 1, job #1167; seed 7, job #1057 | 1) An electric water heater job (#1167) is classed plumbing / water_heater_elec. 2) Water heater replacement normally needs a permit, but the app never asks about permits. 3) The card goes to handyman B0303, who holds no licence. Seed 7: a gas water heater (#1057) card goes to handyman B0306, against the no-gas rule. | Permit-type job list (water heaters, panels, new circuits, re-roofs and the like) that is never sent to handymen. A gas keyword list overrides the AI. The `permit_question` and `handyman_disclosure` fixes on by default. |
| 9 | High | First text lacks disclosures; quiet hours broken for out-of-state phones (T1 20/run; T2 13/run) | S01, seed 1: +19285000008 (T1); day 2 08:00 to +14155000006, local 07:00 (T2) | T1: 1) A homeowner's first message is 1–2 words. 2) The app replies "Hi, this is Havasu Home Quotes. Tell us a bit more…" with no STOP/HELP, rates, frequency, terms or 'business pays us' wording. 3) That is the number's first text. T2: 1) Quiet hours are applied in Arizona time. 2) The 08:00 Arizona batch reached a 415 phone at 07:00 its local time. | Send the full disclosure text as the first message in every path. Apply quiet hours in the recipient's area-code zone, using the stricter of Arizona time and that zone. |
| 10 | Medium | Late safety text (S1 3.0/run, 48/50 runs) | S01, seed 1, job #1081, day 6 17:16 | 1) "breaker panel is sparking breaker keeps tripping". 2) The keyword list has "sparks" and the match is word-prefix, so "sparking" is missed. 3) The consent text goes first. 4) The safety text is sent at 17:24, after the homeowner replies OK and the AI flags it: 8 minutes. | Match word stems (spark, smok, burn, flood, gas, hot, buzz, melt, plus Spanish equivalents). Send the safety text before the consent text. |
| 11 | Medium | Restore re-sends texts (R3 1.0/run, 20/50 runs) | S12, seed 3, job #1182, day 30 | 1) Lost window: day 30 11:00–14:00. 2) At 12:49 the business gets "Job #1182 is no longer active". 3) After the 14:00 restore, the replay re-runs the pick and sends the same text again. | During replay, check Twilio's sent log by (job, template, recipient) before any send. Message IDs should be derived the same way. |
| 12 | Medium | Second free first win through a second name (M3 0.66/run) | S01, seed 1, B0340 / B0340D (jobs #1080, #1220) | 1) The business signs up again under a DBA with another phone. 2) It is unlicensed, so the entity key is its *name*. 3) The name differs, so it gets a second free introduction. | Key free wins on ROC number OR city-list entity OR card/bank fingerprint, checked when the free win is used. |
| 13 | Medium | Books differ from Stripe (M4 0.24/run) | S01, seed 8 (books $2,694 vs Stripe $2,719) | 1) Stripe records a successful charge that the app's ledger never booked. 2) The nightly reconcile raises an 'unknown charge' alert but posts nothing and refunds nothing. | Reconcile from Stripe's event list. Auto-book or auto-refund the charge (refund if the job is closed), and keep the alert. |
| 14 | Medium | Losing businesses not told (H7 23/run) | S01, seed 1, job #1054, B0134 | 1) 'No longer active' goes out once, at pick time, only to businesses that had quoted by then. 2) Quotes are still accepted after the pick (Awaiting confirm / Payment pending). 3) Anyone who quotes later, like B0134, is never told. | Stop accepting quotes once a pick is confirmed, or send 'no longer active' to every quoter whenever the job ends. |
| 15 | Medium | Update texts closer than 3 hours (H2 26/run); list numbers move | S01, seed 1, job #1097, day 10 | 1) An update is held overnight and delivered at 08:00. 2) The 3-hour gap is counted from when the app queued it, so the next update goes at 10:24 (failed) and 10:26 (delivered): 2.4 h after the last one the homeowner received. 3) The list is re-sorted by price, so 'Canyon Pool 77' moves from #1 to #2 between texts, and '1 YES' could pick a different business than the homeowner meant. | Count the gap from delivery time. Keep the list in arrival order (the `stable_list_numbers` fix) or confirm the business name before the pick. |
| 16 | Medium | Business texts and questions not answered (B1 2.3/run; B4 0.58/run) | S01, seed 1, day 52 11:57 (B1); seed 7, F0380 day 33 16:07 (B4) | 1) "Do you take handymen? want work" doesn't match the business phrase list, so no sign-up link is sent. 2) "picked? what do I do" goes to the Needs-you list instead of an automatic reply. | Widen the business-intent phrases, with an AI second opinion on scrubbed text. Auto-reply to any free-text message from a business with a live pick. |
| 17 | Low | Details text repeats the first name | S01, seed 1, job #1092 ("Ruth, Ruth, 2918 McCulloch Blvd") | The details template prints the first name twice. | Fix the template. |
| 18 | Low | Handyman quote ≥$1,000 on an unlicensed category shown (X4 0.10/run) | S01, seed 16, job #1216 | 1) A pressure-wash job (no licence needed). 2) River Handyman 302 quotes $2,408. 3) The refusal rule applies only to handyman or licensed categories, so it is shown, picked, and $199 is charged. | Legal if the work includes no sealing or painting (R-34). Ask the attorney whether handyman-type businesses should be capped across all categories. |

## 4. Checks table (Part 11)

**How to read this.** Each check is judged only from outside-world records: texts actually delivered, Stripe objects, connected calls, AI input logs, and what simulated people did and knew. **PASS** means zero failures in all 24 scenarios × 50 seeds. Columns:
- **Opps:** opportunities per run in S01.
- **S01 f/run:** S01 failures per run.
- **runs:** S01 runs with a failure, out of 50.
- **Scen. failing:** scenarios with at least one failure, out of 24.
- **Worst:** the scenario with the most failures per run.

All [M, sim].

| Check | Result | Opps (S01) | S01 f/run | runs /50 | Scen. failing | Worst | Example (seed: what the outside world saw) |
|---|---|---|---|---|---|---|---|
| M1 charge only after own-phone yes + timely confirm, live job, once, real intro | **FAIL** | 49 | 1.52 | 42 | 23/24 | S02 59.26 | seed 1: #1126: charged day 17 09:52 after homeowner cancelled at day 15 20:15 |
| M2 fee matches tier (recurring rule, ranges, trip tier) | **FAIL** | 49 | 0.04 | 2 | 20/24 | S02 1.66 | seed 5: #1262 electrical quote {'kind': 'trip', 'amount': 129}: charged $75, tier says $25 |
| M3 free first win once per entity | **FAIL** | 33 | 0.66 | 21 | 24/24 | S02 7.24 | seed 1: owner B0340: 2 free introductions (B0340 #1080, B0340D #1220) |
| M4 books match Stripe to the cent | **FAIL** | 1 | 0.24 | 12 | 24/24 | S02 0.84 | seed 8: books $2694.00 vs Stripe net $2719.00 (diff $-25.00) |
| M5 refunds by rule exactly once; disputes handled; failed refunds retried | **FAIL** | 49 | 0.10 | 5 | 24/24 | S02 6.00 | seed 22: #1141: homeowner replied NO (never contacted) yet $199 kept |
| M6 money for a closed job refunded | **FAIL** | 2 | 0.40 | 18 | 23/24 | S02 13.34 | seed 1: #1126: homeowner cancelled at day 15 20:15 before any contact; $25 kept |
| P1 details only to confirmed, paid, licensed business | **FAIL** | 78 | 1.90 | 29 | 24/24 | S02 38.92 | seed 1: #1221: homeowner details sent to F0380 at day 36 17:57 (FAKE business) |
| P2 cards: no PII, no identifying photos | **FAIL** | 721 | 19.90 | 50 | 24/24 | S02 618.14 | seed 1: #1056 card to B0247: contains first name: 'New job #1056: boat repair / boat repair. boat outboard motor won't start (as |
| P3 AI gets no homeowner name/phone/address | **FAIL** | 602 | 68.44 | 50 | 24/24 | S02 1571.86 | seed 1: day 2 11:51 read_request: Claude received first name of a homeowner: 'boat outboard motor won't start This is Ruth Herna |
| P4 homeowner phone never given out | **FAIL** | 1,297 | 0.06 | 3 | 22/24 | S02 168.12 | seed 4: text to B0006: homeowner phone in 'Your card was declined for #1350. Update it at hhq.example/pay/B0006 within 1 busines |
| T1 first text has every disclosure; only people who contacted us | **FAIL** | 319 | 19.94 | 50 | 24/24 | S02 451.80 | seed 1: first text to homeowner +19285000008: missing ['stop', 'help', 'rates may apply', 'frequency', 'terms', 'pays us']: 'Hi, |
| T2 quiet hours (recipient local time) | **FAIL** | 3,138 | 12.98 | 50 | 24/24 | S02 360.48 | seed 1: day 2 08:00 text to +14155000006 (out-of-state, local 07:00): 'Your first quote is in for #1047: 1) Mesa Handyman 303 $2 |
| T3 STOP / plain-language opt-out / CANCEL: exactly one confirmation, then silence | **FAIL** | 8 | 0.20 | 8 | 24/24 | S02 4.10 | seed 1: +19285000168 sent 'alto, no quiero mensajes' at day 39 06:05: 0 confirmations, 4 other texts after (first: '¿En qué códi |
| T4 HELP answered within 5 minutes | **FAIL** | 4 | 0.04 | 2 | 22/24 | S07 2.28 | seed 13: +12065000089 texted help at day 35 23:51: no help reply within 5 min |
| T5 no marketing; links only on our domain | **FAIL** | 3,186 | 2.76 | 46 | 24/24 | S02 57.10 | seed 1: text to +19285000087: off-domain link https://roc.az.gov/file-complaint.: 'Sorry to hear that. You can reach Canyon Pool |
| T6 Spanish speakers get Spanish texts | **FAIL** | 66 | 1.62 | 25 | 24/24 | S02 46.04 | seed 6: Spanish speaker +16125000144 got English at day 33 14:19: 'Lake Cleaning 191 no pudo tomar #1203. Seguimos buscando.' |
| T7 opt-out alarms (3% homeowners / 5% businesses) | **FAIL** | 8 | 0.24 | 12 | 24/24 | S02 2.98 | seed 2: week 1: homeowner opt-out rate 3.0% (>3%), no alarm to Casey |
| S1 safety text with reason within 5 minutes | **FAIL** | 8 | 3.00 | 48 | 24/24 | S02 66.76 | seed 1: #1081 (direct: 'breaker panel is sparking') safety text after 8 min |
| S2 no card before homeowner says safe | **FAIL** | 8 | 0.04 | 2 | 21/24 | S02 1.00 | seed 5: #1220: card to B0051 at day 33 11:59 before homeowner said SAFE |
| S3 emergency cards only to after-hours businesses | **FAIL** | 2 | 0.18 | 7 | 24/24 | S02 5.00 | seed 21: #1079 EMERGENCY card to B0031 (no after-hours service) at day 9 11:43 |
| X1 every card to a covering licence, and to every qualified business | **FAIL** | 204 | 12.08 | 49 | 24/24 | S02 323.94 | seed 1: #1167 (plumbing/water_heater_elec) card to B0303 (handyman, no licence) |
| X2 weed control only to pest licensees | **FAIL** | 204 | 0.00 | 0 | 6/24 | S06 1.60 | seed 8: #1202 weed control card to B0138 (landscape, no pest licence) |
| X3 gas work never to a handyman | **FAIL** | 204 | 0.38 | 13 | 24/24 | S02 8.48 | seed 7: #1057 gas job (water_heater_gas) card to handyman B0306 |
| X4 handyman quotes $1,000+ refused | **FAIL** | 28 | 0.10 | 4 | 23/24 | S13 1.72 | seed 16: homeowner shown handyman quote $2408 from River Handyman 302: 'Your first quote is in for #1216: 1) River Handyman 302 $ |
| X5 tenant and Spanish tags; skip-tenant honoured | **FAIL** | 204 | 5.92 | 38 | 24/24 | S02 230.60 | seed 1: #1252 tenant job card to B0187 lacks tenant tag |
| H1 first-quote text arrives | **FAIL** | 141 | 0.24 | 12 | 24/24 | S06 6.02 | seed 4: #1343: 2 business(es) quoted, homeowner never got a quote list |
| H2 at most one quote update every 3 hours | **FAIL** | 165 | 25.78 | 50 | 24/24 | S02 540.46 | seed 1: #1097: quote updates at day 10 08:00 and day 10 10:26 (2.4 h apart) |
| H3 the 9 p.m. promise is kept | **FAIL** | 207 | 0.02 | 1 | 21/24 | S17a 1.80 | seed 36: #1214: promised news by 9 p.m. day 40; nothing by then |
| H4 every request hears back within 24 hours | **FAIL** | 255 | 5.38 | 50 | 24/24 | S07 139.66 | seed 1: request at day 5 13:32 from +12065000030 ('need monthly yard maintenance'): no answer in 24 h; last text 'Hi, this is Ha |
| H5 forwarded links can't pick | **PASS** | 120 | 0.00 | 0 | 0/24 | S01 0.00 | — |
| H6 'please cancel' / 'never mind' closes the job | **FAIL** | 7 | 1.88 | 43 | 24/24 | S02 46.40 | seed 1: +19285000063 said 'please cancel, I found someone' at day 15 20:15; job kept going: 'Your first quote is in for #1126: 1 |
| H7 losers told 'no longer active' (and only real losers) | **FAIL** | 77 | 23.26 | 50 | 24/24 | S02 480.64 | seed 1: #1054: B0134 quoted but was never told the job is no longer active |
| B1 sign-up recognised from a text | **FAIL** | 9 | 2.30 | 42 | 24/24 | S02 54.04 | seed 1: 'Do you take handymen? want work' from +19286000006 at day 52 11:57: no sign-up link |
| B2 licence read back | **FAIL** | 21 | 2.46 | 43 | 24/24 | S06 7.10 | seed 1: B0019 signed up with ROC300019 at day 0 14:24: licence never read back |
| B3 identity chain text -> voice -> email -> letter; fakes stopped | **FAIL** | 21 | 0.24 | 12 | 24/24 | S08 0.54 | seed 1: F0380 (FAKE unlicensed identity) activated and received homeowner details |
| B4 pick questions answered | **FAIL** | 18 | 0.58 | 24 | 24/24 | S02 11.90 | seed 7: F0380 asked 'picked? what do I do' at day 33 16:07: no answer in 10 min |
| B5 staff phones can link / reach the homeowner | **FAIL** | 4 | 1.00 | 25 | 24/24 | S02 13.60 | seed 2: staff phone of B0100 called at day 9 07:29 for #1082: not linked, no PIN, call dropped |
| B6 complaints pause only at 3 in 90 days | **FAIL** | 3 | 0.00 | 0 | 4/24 | S02 0.20 | seed 6: B0091D: 3 complaints by day 295 14:07 but still got cards |
| B7 tier shading flagged | **FAIL** | 0 | 0.00 | 0 | 6/24 | S05 0.02 | seed 5: B0082 shaded 3 quotes under a tier line; Casey never saw a flag |
| C1 masked calls reach the right person | **FAIL** | 119 | 7.06 | 42 | 24/24 | S02 243.60 | seed 1: day 9 09:53 call from B0034 meant for #1092 rang +13125000045 (route 'most_recent') |
| C2 PIN works | **FAIL** | 52 | 4.08 | 43 | 24/24 | S02 135.06 | seed 1: day 10 13:57 PIN 7548 for #1092 did not reach the homeowner (route most_recent) |
| C3 lines close on cancel / no-show / complaint | **FAIL** | 119 | 0.62 | 11 | 24/24 | S02 16.02 | seed 8: #1171: line still connected a call day 24 08:14 after cancel at day 23 00:27 |
| N1 every job and payment moves forward or ends | **FAIL** | 318 | 25.88 | 50 | 24/24 | S02 599.14 | seed 1: #1086 from day 8 08:02 never reached an end; last text: 'For #1086: what's the street address and your first name? We' |
| N2 no timer fires on a moved-on job | **FAIL** | 2,669 | 0.04 | 2 | 22/24 | S06 0.46 | seed 8: #1128: 'For #1128: what's the street address and your first name? We' sent day 15 15:45 after the job closed at day 15 1 |
| K2 alerts unanswered 24 h go to backup + safe mode | **PASS** | 3 | 0.00 | 0 | 0/24 | S01 0.00 | — |
| R1 job numbers never reused | **PASS** | 288 | 0.00 | 0 | 0/1 | S12 0.00 | — |
| R3 nothing sent twice across the restore | **FAIL** | 1,461 | 1.00 | 20 | 1/1 | S12 1.00 | seed 3: 'Job #1182 is no longer active. Thanks for quoting.' to +19287000180 sent at day 30 12:49 and again day 30 14:00 (after  |
| R2 every kept charge matches a pick | **FAIL** | 48 | 1.52 | 43 | 1/1 | S12 1.52 | seed 1: #1126: charged day 17 09:52 after homeowner cancelled at day 15 20:15 |

**Totals: 3 PASS, 45 FAIL out of 48 judged checks** [M, sim]. R1 and R3 are judged only in S12; R2 equals M1 there. K1 (Casey's minutes per week) is a measurement, reported in section 7. Low opportunity counts (B6, B7, X2) mean a PASS is weak evidence. With every fix on (S17a), the remaining failures per run were: M3 0.60, P1 0.74, P2 1.08, P3 7.48, T1 34.68, T2 2.26, T5 1.92, T6 1.78, S1 1.44, X1 5.56, X5 4.52, H2 22.24, H3 1.80, H4 2.06, H7 17.24, B1 2.04, B2 2.16, B4 0.64, B5 0.80, C1 0.74, C2 1.82, N1 21.14. T1 got *worse* under the fixes (the lenient-consent fix sends a short first text without disclosures), so don't ship that fix.

**Per-scenario failure totals** (sum of failures per run across all checks; hot spots) [M, sim]:

| Scenario | total f/run | top 3 checks |
|---|---|---|
| S01_normal | 254 | P3 68.4, N1 25.9, H2 25.8 |
| S02_growth | 6320 | P3 1571.9, P2 618.1, N1 599.1 |
| S03_busy_summer | 886 | P3 237.5, C1 96.5, N1 85.8 |
| S04_snowbird | 268 | P3 67.6, T2 29.9, H2 27.1 |
| S05_boat_season | 256 | P3 68.1, H2 26.8, H7 26.0 |
| S06_vendor_chaos | 427 | P3 94.7, X1 71.2, P2 54.5 |
| S07_twilio_suspend | 402 | H4 139.7, N1 124.6, P3 36.7 |
| S08_adversarial | 289 | P3 81.0, H2 29.5, H7 27.3 |
| S09_casey_away | 250 | P3 69.9, N1 25.7, H2 24.8 |
| S10_dispute_wave | 252 | P3 69.0, N1 25.4, H2 24.9 |
| S11a_collide_fwd | 840 | P3 231.0, N1 85.2, H2 75.0 |
| S11b_collide_rev | 842 | P3 230.4, N1 84.9, H2 75.0 |
| S12_db_restore | 253 | P3 69.5, N1 25.7, H2 25.6 |
| S13_crowded | 487 | H7 87.5, H2 70.0, P3 66.6 |
| S14_thin | 217 | P3 71.4, N1 26.6, T1 20.5 |
| S15_spanish20 | 253 | P3 65.7, N1 27.7, H2 23.8 |
| S17a_all_fixes | 137 | T1 34.7, H2 22.2, N1 21.1 |
| S17b_approved_scope | 231 | P3 69.7, N1 26.5, T1 19.2 |
| S17c_weekday_window | 297 | P3 69.3, P4 44.2, H2 25.5 |
| S17d_roc_lists | 245 | P3 68.3, N1 26.3, H2 25.1 |
| S17e_insurance_req | 227 | P3 69.2, N1 25.6, T1 20.5 |
| S17f_rec_partial | 254 | P3 68.5, N1 25.9, H2 25.6 |
| S17g_roc_layout | 255 | P3 68.9, N1 25.9, H2 25.8 |
| S17h_10_per_day | 512 | P3 143.1, N1 52.0, H2 49.4 |

## 5. Money (Part 12)

**Given [G]:**
- fees $25 / $75 / $199 at the $500 and $1,500 lines;
- recurring services count as 2 months;
- Stripe 2.9% + 30¢ (bank 0.8%, capped at $5), $15 per dispute, Stripe keeps its fee on refunds;
- fixed costs $157 a month;
- Twilio $0.0083 per segment + $0.004 carrier fee, MMS $0.022, Verify $0.05, calls $0.0085 in / $0.014 out per minute;
- letters $1;
- start cash $1,000 + $500 Stripe reserve;
- Normal demand 5 requests a day.

**Assumed [A]:**
- the price-reaction curves (section 6);
- homeowner and business behaviour rates (config.py);
- job value lognormal with sigma 0.6 [G suggestion, kept];
- Stripe pays out 2 days after a charge;
- the $500 reserve is not spendable.

Everything else is **[M, sim]**.

### 5.1 Normal (S01), by month [M, sim; mean of seeds 1–50]

| Month | requests | picks | paid_intros | free_wins | n_refunds | revenue | refunds | stripe_fees | dispute_fees | twilio_sms | twilio_verify | twilio_voice | claude | letters | email | fixed | net | cost_per_request | avg_fee |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 146 | 56 | 16 | 21 | 0.7 | 837 | 33 | 25 | 0 | 26 | 4 | 6 | 4 | 1 | 0 | 157 | 583 | 1.52 | 50 |
| 2 | 144 | 63 | 32 | 8 | 1.5 | 1,684 | 78 | 49 | 0 | 27 | 2 | 7 | 3 | 0 | 0 | 157 | 1,360 | 1.72 | 52 |

`twilio_sms` includes MMS and the number and 10DLC fees [G]. `email` costs $0 at this volume [A, free tier]. `cost_per_request` = vendor + fixed costs divided by requests.

### 5.2 Growth (S02, 24 months), by month [M, sim; mean of seeds 1–50]

Demand ramps from 1/day to 10/day by month 12 and 16/day by month 24 [G]. Businesses join over time [A]. Month 24 is normalised to 30 days.

| Month | requests | picks | paid_intros | free_wins | n_refunds | revenue | refunds | stripe_fees | dispute_fees | twilio_sms | twilio_verify | twilio_voice | claude | letters | email | fixed | net | cost_per_request | avg_fee |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 41 | 16 | 2 | 10 | 0.1 | 97 | 2 | 3 | 0 | 8 | 3 | 2 | 1 | 1 | 0 | 157 | -79 | 4.34 | 46 |
| 2 | 64 | 29 | 10 | 9 | 0.3 | 474 | 18 | 14 | 0 | 12 | 1 | 3 | 2 | 0 | 0 | 157 | 267 | 3.01 | 48 |
| 3 | 84 | 41 | 19 | 7 | 0.5 | 865 | 20 | 26 | 0 | 17 | 1 | 4 | 2 | 0 | 0 | 157 | 637 | 2.49 | 47 |
| 4 | 106 | 51 | 27 | 5 | 0.9 | 1,370 | 55 | 40 | 0 | 21 | 1 | 5 | 3 | 0 | 0 | 157 | 1,088 | 2.16 | 51 |
| 5 | 129 | 61 | 35 | 5 | 1.5 | 1,831 | 71 | 54 | 0 | 27 | 2 | 7 | 3 | 0 | 0 | 157 | 1,512 | 1.93 | 53 |
| 6 | 149 | 68 | 41 | 4 | 1.7 | 2,095 | 112 | 62 | 0 | 31 | 2 | 7 | 4 | 0 | 0 | 157 | 1,721 | 1.77 | 51 |
| 7 | 172 | 81 | 48 | 4 | 2.2 | 2,497 | 112 | 73 | 0 | 36 | 2 | 9 | 4 | 0 | 0 | 157 | 2,103 | 1.64 | 52 |
| 8 | 191 | 85 | 51 | 3 | 2.1 | 2,825 | 114 | 82 | 0 | 40 | 2 | 9 | 5 | 0 | 0 | 157 | 2,415 | 1.55 | 54 |
| 9 | 214 | 92 | 56 | 3 | 3.2 | 3,166 | 178 | 92 | 0 | 44 | 2 | 10 | 5 | 0 | 0 | 157 | 2,677 | 1.45 | 56 |
| 10 | 239 | 96 | 58 | 3 | 3.0 | 3,232 | 176 | 95 | 0 | 48 | 3 | 10 | 6 | 0 | 0 | 157 | 2,737 | 1.34 | 55 |
| 11 | 264 | 96 | 58 | 3 | 2.8 | 3,208 | 150 | 92 | 0 | 53 | 3 | 10 | 6 | 0 | 0 | 157 | 2,738 | 1.21 | 55 |
| 12 | 284 | 99 | 61 | 3 | 3.0 | 3,474 | 184 | 97 | 0 | 56 | 3 | 11 | 7 | 0 | 0 | 157 | 2,961 | 1.16 | 57 |
| 13 | 304 | 117 | 71 | 3 | 3.2 | 3,783 | 169 | 108 | 0 | 63 | 3 | 12 | 7 | 0 | 0 | 157 | 3,263 | 1.16 | 53 |
| 14 | 311 | 127 | 80 | 3 | 4.4 | 4,067 | 227 | 119 | 0 | 68 | 3 | 14 | 8 | 0 | 0 | 157 | 3,472 | 1.19 | 51 |
| 15 | 332 | 129 | 84 | 2 | 5.4 | 4,421 | 273 | 129 | 0 | 72 | 3 | 15 | 8 | 0 | 0 | 157 | 3,765 | 1.16 | 53 |
| 16 | 347 | 131 | 83 | 3 | 4.4 | 4,316 | 239 | 126 | 0 | 75 | 3 | 14 | 8 | 0 | 0 | 157 | 3,693 | 1.11 | 52 |
| 17 | 358 | 137 | 86 | 2 | 4.7 | 4,249 | 233 | 124 | 0 | 78 | 3 | 15 | 9 | 0 | 0 | 157 | 3,631 | 1.08 | 50 |
| 18 | 375 | 135 | 85 | 2 | 5.2 | 4,424 | 280 | 129 | 0 | 80 | 3 | 15 | 9 | 0 | 0 | 157 | 3,751 | 1.05 | 52 |
| 19 | 383 | 131 | 82 | 2 | 4.9 | 4,284 | 233 | 127 | 0 | 82 | 3 | 14 | 9 | 0 | 0 | 157 | 3,658 | 1.03 | 52 |
| 20 | 403 | 136 | 86 | 2 | 5.0 | 4,699 | 247 | 139 | 0 | 86 | 4 | 15 | 10 | 0 | 0 | 157 | 4,042 | 1.02 | 55 |
| 21 | 417 | 120 | 74 | 2 | 4.2 | 3,962 | 234 | 116 | 0 | 84 | 3 | 13 | 10 | 0 | 0 | 157 | 3,346 | 0.92 | 53 |
| 22 | 429 | 117 | 72 | 2 | 4.8 | 3,950 | 281 | 117 | 0 | 85 | 3 | 13 | 10 | 0 | 0 | 157 | 3,283 | 0.90 | 54 |
| 23 | 451 | 117 | 71 | 2 | 4.9 | 3,902 | 270 | 115 | 0 | 88 | 3 | 12 | 11 | 0 | 0 | 157 | 3,245 | 0.86 | 54 |
| 24 | 463 | 117 | 73 | 2 | 4.8 | 3,949 | 258 | 118 | 0 | 91 | 3 | 12 | 11 | 0 | 0 | 118 | 3,337 | 0.76 | 54 |

**Growth milestones** [M, sim]:
- **$20k/yr pace (≥ $1,667/month):** first reached in month **6** of the mean path. Per seed: median month 6, range 4–11.
- **$50k/yr pace (≥ $4,167/month):** the mean path never reaches it in 24 months. 43/50 seeds touch it in at least one month (median month 15), but few stay there.
- **$20k or $50k a MONTH:** never reached on any seed.
- **24-month total profit:** $64,374 (p10–p90 $52,826–$73,925).
- **Businesses:** 127 joined, 48 active at the end.
- **Late decline:** monthly net falls in months 21–23 as churn outpaces new sign-ups. See the churn table in section 6.

### 5.3 Requests per day needed (demand sweep; 120-day runs, seeds 1–8; steady state = months 3–4) [M, sim]

| Requests/day | Steady monthly net | Annualised | Businesses kept |
|---|---|---|---|
| 2 | $511 | $6,212 | 52 |
| 3 | $980 | $11,921 | 53 |
| 5 | $1,777 | $21,617 | 53 |
| 8 | $2,593 | $31,548 | 49 |
| 12 | $3,526 | $42,903 | 48 |
| 16 | $4,529 | $55,098 | 46 |
| 25 | $6,514 | $79,260 | 44 |
| 40 | $9,295 | $113,093 | 43 |
| 60 | $10,853 | $132,041 | 40 |
| 100 | $11,798 | $143,547 | 34 |

- **$20,000 a year:** about **4.7 requests/day** (interpolated).
- **$50,000 a year:** about **14.3 requests/day**.
- **$20,000 or $50,000 a MONTH:** not reached at any tested level. Monthly profit levels off near $11,798, because every local business already gets every card and businesses leave faster at high volume. A straight-line projection (435 and 1,687/day) is not meaningful.

### 5.4 Every scenario, per 60-day run [M, sim; mean of seeds 1–50]

| Scenario | Profit | p10 | p90 | Paid intros | Free wins | Refunds n | Refunds $ | Broken promises | Casey min/wk | Low cash mean | Low cash worst (seed) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| S01_normal | $1,943 | $960 | $2,967 | 49 | 29 | 2.2 | $111 | 7.5 | 77 | $822 | $726 (47) |
| S02_growth | $64,374 | $52,826 | $73,925 | 1438 | 88 | 78.6 | $4,222 | 171.3 | 85 | $729 | $574 (20) |
| S03_busy_summer | $9,453 | $6,044 | $14,417 | 200 | 36 | 16.5 | $925 | 27.6 | 108 | $822 | $755 (8) |
| S04_snowbird | $1,982 | $1,230 | $3,005 | 52 | 29 | 2.7 | $119 | 7.9 | 75 | $823 | $552 (47) |
| S05_boat_season | $2,667 | $1,722 | $3,594 | 55 | 29 | 2.3 | $139 | 8.3 | 80 | $826 | $751 (40) |
| S06_vendor_chaos | $1,216 | $542 | $1,985 | 36 | 27 | 2.0 | $108 | 16.7 | 113 | $808 | $701 (47) |
| S07_twilio_suspend | $429 | -$57 | $927 | 17 | 21 | 0.7 | $39 | 145.4 | 202 | $814 | $593 (31) |
| S08_adversarial | $1,955 | $1,138 | $2,989 | 53 | 30 | 2.7 | $130 | 10.2 | 82 | $826 | $767 (6) |
| S09_casey_away | $1,795 | $765 | $2,671 | 46 | 28 | 2.0 | $109 | 7.6 | 130 | $822 | $728 (47) |
| S10_dispute_wave | $1,911 | $1,033 | $2,743 | 49 | 29 | 1.9 | $91 | 7.9 | 96 | $820 | $679 (47) |
| S11a_collide_fwd | $8,489 | $6,285 | $11,018 | 194 | 38 | 11.5 | $604 | 26.2 | 106 | $817 | $658 (39) |
| S11b_collide_rev | $8,716 | $6,369 | $11,621 | 195 | 38 | 11.7 | $571 | 25.3 | 109 | $821 | $751 (8) |
| S12_db_restore | $1,925 | $1,047 | $2,839 | 48 | 29 | 1.9 | $98 | 7.8 | 78 | $822 | $726 (47) |
| S13_crowded | $3,489 | $2,673 | $4,305 | 88 | 77 | 3.1 | $156 | 9.3 | 126 | $814 | $777 (8) |
| S14_thin | $1,282 | $716 | $2,180 | 35 | 19 | 1.3 | $62 | 7.8 | 67 | $829 | $776 (22) |
| S15_spanish20 | $1,937 | $1,115 | $2,875 | 49 | 29 | 1.6 | $92 | 7.8 | 75 | $824 | $741 (21) |
| S17a_all_fixes | $1,408 | $843 | $1,963 | 41 | 26 | 3.7 | $194 | 4.2 | 316 | $802 | $660 (37) |
| S17b_approved_scope | $1,864 | $662 | $2,964 | 40 | 8 | 2.2 | $148 | 8.7 | 65 | $831 | $662 (38) |
| S17c_weekday_window | $2,052 | $1,251 | $2,906 | 51 | 29 | 2.0 | $105 | 7.5 | 76 | $823 | $750 (47) |
| S17d_roc_lists | $1,908 | $1,187 | $2,605 | 48 | 29 | 2.1 | $105 | 7.9 | 70 | $822 | $749 (47) |
| S17e_insurance_req | $1,478 | $831 | $2,164 | 41 | 23 | 1.5 | $70 | 7.2 | 70 | $825 | $700 (47) |
| S17f_rec_partial | $1,915 | $960 | $2,842 | 49 | 29 | 3.3 | $138 | 7.5 | 77 | $822 | $726 (47) |
| S17g_roc_layout | $1,897 | $945 | $2,842 | 49 | 29 | 2.2 | $114 | 7.6 | 87 | $822 | $726 (47) |
| S17h_10_per_day | $4,948 | $3,196 | $6,993 | 116 | 35 | 5.7 | $299 | 15.7 | 89 | $827 | $803 (8) |

- **'Broken promises'** counts outside-world failures of the four homeowner promises: first-quote text (H1), the 9 p.m. promise (H3), a reply within 24 h (H4) and cancel honoured (H6).
- **Lowest cash point** across every scenario and seed: **$552** (S04_snowbird, seed 47), measured against $1,000 operating cash. The $500 Stripe reserve is held separately and never needed [A]. Cash never goes negative in any run [M, sim], because early free wins cost only vendor fees.
- **30-day texting suspension (S07 vs S01, same seeds):**
  - profit −$1,514 per 60-day run;
  - Casey +124 min/week, about **18 extra hours** over the run;
  - broken promises +138.

  [M, sim]


## 6. Price reaction (Part 9)

### Rules used (all [A] unless marked)
- **Confirm and pay:** p = 1 / (1 + (s/s50)^3) [G formula], where s = fee ÷ job value (first-year value for recurring jobs). s50 is set to 0.10, 0.15 or 0.20 [G]. A free first win has s = 0. I think the curve is reasonable for one-off jobs but too harsh for recurring work, where the business values the client beyond year one. The 4-month recurring variant (alt5) partly tests this.
- **Joining:** each business in the local pool joins with a base probability of U(15%, 30%) [G range]. That is scaled by how the expected fee compares with its cost to win a customer elsewhere (Part 6.3 [G]): factor = (1 − fee/CAC) / (1 − base fee/CAC), clipped to 0–1.3. The factor is ×0.95 when the free win applies only in the first 90 days. Businesses join through launch and growth [A].
- **Leaving:** checked monthly.
  - **Fee strikes:** a fee counts as a strike when it is above s50/2 of the job value. After 3 strikes there is a 50% chance of quitting.
  - **No wins:** 2 months with no win gives a 15% chance of quitting.
  - **Background:** 1.5% a month.
  - **STOP:** half of leavers text STOP; the rest go quiet.
- **Quoting:** the chance of quoting a card is base × (0.5 + 0.5 × min(1, win rate/0.25)), with win rate = (wins+1)/(quotes+4). So in crowded categories businesses quote less.
- **Gaming:** shaders quote just under $500 or $1,500, and trip-fee gamers quote a visit fee. They make up 5% of businesses (doubled in S08) [G share]. The fee avoided is measured. A gamer counts as caught if the owner-portal shading flag fires or the homeowner replies PROBLEM at the day-3 check-in.
- **Homeowners:** the chance of picking rises with quotes per job, 0.85 × (1 − 0.35^n). A past homeowner returns with a new request at a 10% repeat rate [A]. The repeat rate was not varied with the outcome, so the model can't show whether more quotes bring people back. That is a gap the hand-run test must fill.

### Every fee structure × s50 (Scenario 16; 60 days; seeds 1–50) [M, sim]

**Structures:** base = $25/$75/$199 [G]; alt1 = $19/$59/$199; alt2 = $25/$75/$300; alt3 = flat $25; alt4 = 5% (min $15, max $250); alt5 = base with recurring counted at 4 months; first90 = base with free wins only in the first 90 days. Columns:
- **Conf.:** share of picks confirmed.
- **Joined/kept:** businesses joined and still active at the end.
- **Q/job:** quotes per job.
- **Pick:** homeowner pick rate.
- **Avg fee:** average fee per paid intro.
- **Gaming:** fee avoided / caught.

| Structure | s50 | Profit | p10–p90 | Conf. | Joined / kept | Q/job | Pick | Avg fee | Casey min/wk | Gaming: fee avoided / caught |
|---|---|---|---|---|---|---|---|---|---|---|
| base | 0.10 | $1,508 | $894–$2,199 | 0.74 | 48 / 47 | 1.33 | 0.55 | $51 | 77 | $59 / 0.38 of 1.22 |
| base | 0.15 | $1,943 | $960–$2,967 | 0.83 | 48 / 47 | 1.34 | 0.57 | $52 | 77 | $56 / 0.18 of 1.10 |
| base | 0.20 | $2,092 | $1,118–$2,938 | 0.88 | 48 / 47 | 1.35 | 0.58 | $51 | 78 | $71 / 0.24 of 1.30 |
| alt1 | 0.10 | $1,442 | $936–$2,119 | 0.79 | 55 / 54 | 1.42 | 0.59 | $40 | 77 | $74 / 0.22 of 1.42 |
| alt1 | 0.15 | $1,726 | $1,032–$2,688 | 0.87 | 55 / 54 | 1.42 | 0.60 | $41 | 76 | $67 / 0.16 of 1.18 |
| alt1 | 0.20 | $1,872 | $1,097–$2,844 | 0.90 | 55 / 54 | 1.42 | 0.61 | $42 | 74 | $76 / 0.22 of 1.34 |
| alt2 | 0.10 | $1,510 | $779–$2,351 | 0.72 | 47 / 46 | 1.30 | 0.56 | $51 | 73 | $54 / 0.24 of 0.80 |
| alt2 | 0.15 | $2,048 | $1,114–$3,311 | 0.83 | 47 / 46 | 1.31 | 0.57 | $54 | 74 | $72 / 0.26 of 1.02 |
| alt2 | 0.20 | $2,142 | $1,130–$3,043 | 0.87 | 47 / 46 | 1.32 | 0.58 | $52 | 74 | $64 / 0.26 of 0.94 |
| alt3 | 0.10 | $824 | $516–$1,220 | 0.79 | 56 / 55 | 1.54 | 0.59 | $25 | 78 | $0 / 0.30 of 0.88 |
| alt3 | 0.15 | $968 | $687–$1,389 | 0.86 | 56 / 55 | 1.53 | 0.61 | $25 | 78 | $0 / 0.38 of 1.06 |
| alt3 | 0.20 | $1,067 | $691–$1,467 | 0.90 | 56 / 55 | 1.53 | 0.62 | $25 | 78 | $0 / 0.36 of 0.92 |
| alt4 | 0.10 | $1,288 | $595–$2,111 | 0.86 | 57 / 56 | 1.51 | 0.60 | $33 | 80 | $43 / 0.46 of 1.32 |
| alt4 | 0.15 | $1,361 | $765–$2,331 | 0.91 | 57 / 56 | 1.52 | 0.61 | $32 | 80 | $39 / 0.48 of 1.38 |
| alt4 | 0.20 | $1,402 | $735–$2,199 | 0.92 | 57 / 57 | 1.52 | 0.61 | $31 | 78 | $37 / 0.42 of 1.22 |
| alt5 | 0.10 | $2,013 | $1,101–$3,000 | 0.71 | 42 / 41 | 1.26 | 0.54 | $67 | 72 | $50 / 0.22 of 0.96 |
| alt5 | 0.15 | $2,348 | $1,439–$3,531 | 0.81 | 42 / 41 | 1.26 | 0.56 | $63 | 72 | $58 / 0.26 of 1.16 |
| alt5 | 0.20 | $2,648 | $1,552–$4,075 | 0.88 | 42 / 41 | 1.26 | 0.57 | $64 | 69 | $51 / 0.26 of 0.98 |
| first90 | 0.15 | $1,998 | $1,251–$2,921 | 0.82 | 46 / 46 | 1.30 | 0.57 | $52 | 73 | $69 / 0.36 of 1.14 |

**What this says** [M, sim]:
- **Most profit at every s50:** alt5 (recurring at 4 months), then alt2 ≈ base.
- **Flat $25 (alt3)** roughly halves profit.
- **alt1 and alt4** keep more businesses and lift confirms, quotes per job and pick rate, but lose money overall.
- **Sensitivity:** moving s50 from 0.15 to 0.10 costs the base structure about $435 per 60 days. That makes the hand-run test's measured confirm rate the single most valuable number to collect.
- **Gaming** is small in money terms (one gamer per run, tens of dollars avoided), and the portal flag plus PROBLEM reply catch only a minority. The $500 and $1,500 cliffs invite shading.

### Churn by category (Growth, 24 months, seeds 1–50) [M, sim]

| Category group | Activated/run | Left/run | Share left | Leavers with 0 wins |
|---|---|---|---|---|
| hvac | 7.3 | 5.8 | 79% | 6% |
| handyman | 2.4 | 1.9 | 78% | 1% |
| plumbing | 6.6 | 5.1 | 78% | 4% |
| electrical | 9.1 | 6.6 | 72% | 12% |
| detail | 9.8 | 7.1 | 72% | 12% |
| painting | 5.2 | 3.7 | 71% | 18% |
| boat | 22.8 | 15.2 | 66% | 29% |
| garage | 1.4 | 0.9 | 65% | 41% |
| pwash | 2.2 | 1.4 | 61% | 46% |
| appliance | 1.7 | 1.1 | 61% | 47% |
| junk | 1.9 | 1.1 | 59% | 21% |
| roof | 6.4 | 3.6 | 56% | 43% |
| pool | 17.6 | 9.6 | 54% | 24% |
| movers | 1.7 | 0.9 | 54% | 40% |
| solar | 2.1 | 1.1 | 53% | 31% |
| pest | 5.6 | 2.8 | 49% | 20% |
| window | 2.2 | 1.1 | 49% | 43% |
| cleaning | 11.2 | 4.8 | 43% | 18% |
| trashcan | 0.4 | 0.1 | 33% | 43% |
| landscape | 19.0 | 5.9 | 31% | 63% |

**Licensed repair trades (HVAC, plumbing, electrical, handyman) lose about 72–79% of the businesses that join within 24 months**, almost all of them *after* winning [M, sim]. The cause is the Assumed leave rule: a $25 fee on a ~$300 repair is about 8% of the job, above the s50/2 = 7.5% strike line, so those trades pile up strikes. Recurring categories (landscape, cleaning, pool) churn least; most of their leavers never won. If real businesses tolerate $25 on small repairs better than this rule assumes, churn is overstated. The hand-run test must measure this.

## 7. Casey's workload (minutes per week by task) [M, sim]

Task minutes are [A] (config.py `casey.minutes`). Casey works the 'Needs you' list only in **two 30-minute windows a day** around his shifts [A]: two weeks of day shifts (06:15 and 19:45), then two weeks of nights (12:00 and 17:00). That caps him at 420 min/week. Items wait while he is on shift. The **backup** column is the backup person's minutes per week during S09's two-week absence.

| Task | S01 | S02 | S03 | S06 | S07 | S08 | S09 | S13 | S17a | S17h | S09 backup (away) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| alert | 11 | 10 | 10 | 32 | 11 | 11 | 10 | 10 | 13 | 9 | 23 |
| approve_signup | 3 | 1 | 3 | 9 | 3 | 3 | 4 | 10 | 208 | 3 | 3 |
| biz_text | 5 | 8 | 12 | 5 | 2 | 5 | 5 | 12 | 4 | 9 | 0 |
| callback_call | 1 | 4 | 4 | 4 | 130 | 2 | 1 | 1 | 1 | 2 | 0 |
| city_mismatch | 5 | 1 | 5 | 5 | 5 | 6 | 6 | 18 | 8 | 5 | 5 |
| complaint | 6 | 11 | 18 | 6 | 3 | 7 | 8 | 13 | 4 | 12 | 0 |
| contact_dispute | 0 | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| dispute | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| ledger | 19 | 20 | 19 | 19 | 19 | 19 | 21 | 19 | 19 | 19 | 0 |
| letter_manual | 5 | 1 | 5 | 5 | 8 | 7 | 6 | 15 | 38 | 5 | 7 |
| pause_review | 2 | 6 | 4 | 9 | 2 | 2 | 48 | 8 | 2 | 3 | 297 |
| refund_fail | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| shade_flag | 0 | 3 | 5 | 1 | 0 | 1 | 0 | 2 | 0 | 2 | 0 |
| spot_check | 19 | 20 | 19 | 19 | 19 | 19 | 21 | 19 | 19 | 19 | 0 |
| **Total** | **77** | **85** | **108** | **113** | **202** | **82** | **130** | **126** | **316** | **89** | **167** |

- **On shift vs away:**
  - Normal load is about 77 min/week, roughly 1.3 hours.
  - The biggest items are pause reviews, the weekly spot-check and the ledger check.
  - When Casey is away (S09) the backup needs about 167 min/week, mostly reviewing paused businesses.
  - Casey's own load averages 130 min/week over S09 because of the catch-up after he returns.
- **Load spikes:**
  - a texting suspension (S07): 202 min/week;
  - a crowded market (S13): 126;
  - all fixes on (S17a): 316 min/week, because the strict identity fixes push many launch sign-ups to manual approval. Make that one fix cheaper (for example, batch approvals) before enabling it.
- **24-month growth:** 85 min/week average.

## 8. Vendor behaviour [M, sim; per 60-day run]

| Scenario | sms | sms_failed | verify | calls | claude | emails | letters | charges | charges_failed | disputes | refunds |
|---|---|---|---|---|---|---|---|---|---|---|---|
| S01_normal | 3,185.6 | 47.7 | 103.5 | 118.8 | 602.3 | 1.9 | 1.1 | 50.7 | 2.0 | 0.0 | 2.2 |
| S06_vendor_chaos | 4,146.0 | 554.2 | 91.5 | 96.8 | 648.5 | 3.1 | 0.8 | 46.2 | 10.4 | 0.0 | 2.0 |
| S07_twilio_suspend | 1,793.9 | 268.5 | 76.7 | 58.6 | 317.5 | 33.5 | 1.2 | 17.1 | 0.4 | 0.0 | 0.7 |
| S10_dispute_wave | 3,185.9 | 49.2 | 103.2 | 120.0 | 603.7 | 1.9 | 1.1 | 50.6 | 1.5 | 5.0 | 1.9 |
| S17g_roc_layout | 3,189.7 | 48.4 | 103.4 | 118.9 | 603.6 | 1.8 | 1.1 | 50.5 | 1.9 | 0.0 | 2.2 |
| S02_growth | 80,394.2 | 1,199.2 | 1,124.5 | 2,392.0 | 13,893.1 | 3.5 | 1.3 | 1,476.8 | 39.2 | 0.0 | 78.6 |

- **Twilio:**
  - Normal: 1.5% of texts fail [A rate, M count].
  - Chaos (S06, all failure rates ×3 [G]): 13% fail. First-quote texts then go missing (H1 6.0/run vs 0.24), and identity codes fail more (B2 7.1/run).
  - The app retries important texts once and has no other channel.
  - Suspension (S07) is the single worst vendor event: N1 125/run and H4 140/run.
- **Stripe:**
  - Declines: 2.0 per run in Normal and 10.4 in Chaos. The 1-business-hour card window works.
  - The 4-hour outage in S06 released picks as designed.
  - Books-vs-Stripe mismatches (M4) come from charges the app never booked; the nightly reconcile only alerts.
  - The S10 dispute wave: 5.0 disputes per run at $15 each [G]. Profit $1,911 vs $1,943, with more Casey time (96 min/week).
- **AI (Claude):**
  - About 602 calls per run, costing $7 per 60 days [M].
  - During the S06 outage, requests queue and keyword safety still fires. But the AI input contains homeowner details in every run (P3), which is the main AI finding.
  - Prompt-injection texts (S08) had no effect on money or details, because only fixed output fields are read.
- **License sites (ROC, Agriculture, city list):**
  - 'Unknown' results (CAPTCHA, down, or a layout change; S17g forces a layout change on day 20) send sign-ups to Casey, and his approval activates them with no identity code (B0019 example). That path, not the scraper, is the weakness.
  - The ROC-list variant (S17d) cut Casey time by about 7.5 min/week at equal profit.
- **Email and mail:** rarely used (identity fall-through only); letter costs are negligible.

## 9. Open decisions (Part 13) [M, sim; same-seed paired differences ± standard error]

| # | Decision | Result | Recommendation |
|---|---|---|---|
| 1 | 24 h window: weekdays only vs every day | S17c − S01 profit +109 ± 61 per 60 d; 365-day: +552 ± 589/yr (not significant) | Weekdays only: a slight edge, more quotes, and fairer to small firms. Make the 9 p.m. promise match the window. |
| 2 | Free first win: all vs first 90 days | 365-day: +1,586 ± 714/yr; businesses kept -4.60 ± 1.65 | First-90-days, if you accept a smaller business pool; otherwise keep it for all until the hand-run test measures joining. |
| 3 | Recurring at 2 vs 4 months | 365-day: +4,072 ± 878/yr; businesses kept -5.70 ± 1.53; confirm -0.02 ± 0.01 | 4 months: the largest profit lever found. Watch pool and landscape churn. |
| 4 | $25/$75/$199 vs alternatives | alt5 − base +405 ± 106; alt2 − base +105 ± 104; alt1 − base -217 ± 101 per 60 d | Keep $25/$75/$199 with recurring at 4 months (= alt5). $300 top tier: no clear gain. Avoid flat $25 and $19. Get a legal view on tiers first (see Licensing). |
| 5 | Partial refund if recurring stops within 30 days | 365-day: -358 ± 536/yr | Not favoured on money; adopt only if the hand-run test shows early stops hurt business trust. |
| 6 | Unlicensed categories: insurance required vs 'on file yes/no' | S17e − S01 profit -465 ± 88; businesses kept -9.84 ± 0.48; active fakes -0.16 ± 0.10 | Show 'insurance on file: yes/no' and fix identity checks (problem 3). Requiring certificates costs profit and businesses; the fake reduction is better achieved by identity fixes. |
| 7 | Live ROC lookups vs daily lists + live check at pick | S17d − S01 profit -35 ± 70; Casey -7.53 ± 1.71 min/wk; fakes -0.08 ± 0.07 | Daily lists + live check at the pick: same money, less Casey time, robust to layout changes (S17g). |

Also measured: all fixes on (S17a) − S01: profit -535 ± 90, Casey +238.98 ± 7.23 min/week, active fakes -0.26 ± 0.09. Approved hand-run scope (S17b) − S01: profit -79 ± 165, businesses kept -39.78 ± 0.96, confirm share -0.25 ± 0.01.

## 10. What the model can't tell us, and what the hand-run test must measure
- **Every behaviour rate is [A].** That includes:
  - confirm-and-pay (s50) and the join and leave rules;
  - quoting rates and homeowner pick and return rates;
  - persona shares (cancellers, absent, adversarial);
  - Casey's task minutes and windows.

  The profit numbers move most with s50 and the leave rule. Measure these, in this order:
  1. the share of picks confirmed at each fee tier;
  2. business reactions to $25 on jobs under $300;
  3. quotes per card and time to first quote;
  4. the homeowner pick rate by number of quotes;
  5. how often homeowners return;
  6. Casey's real minutes per task.
- **Demand is [A]:** the 5/day Normal rate and the growth ramp are given, not forecast. Nothing here says Lake Havasu City will send 5 requests a day. Ask Hava is zero [G].
- **Legal questions can't be simulated:** the §32-1101 contracting risk, the handyman limit, pest-control routing, the tier-fee structure, A2P and TCPA wording all need an attorney before the gate.
- **The fake vendors are simplified:** real Twilio carrier filtering, real Stripe Radar and 3-D Secure flows, real ROC page layouts and AI mistakes differ. The vendor failure rates are [A] from Part 7.
- **The AI is modelled as reading correctly most of the time, with a fixed error rate.** Real misclassification patterns, especially permit and gas detection, need a labelled sample from the hand-run test.
- **Privacy checks are pattern-based:** the PII detector is a set of rules on texts and AI inputs. It may miss oddly written personal details, so the real exposure could be higher than measured.
- **Fraud and gaming are a 5% share [G], modelled simply.** Real fee bypass, where the business and homeowner meet off-platform, can't be seen from outside events and is not measured.
- **The 2-week Casey absence and the backup person's speed are [A].**

## 11. Code

**Where it is.** The full code is about 250 KB, which does not fit in a report under 90 KB. It is all in `sim_v93/` and inside `sim_v93_results.zip` (code, configs and every raw result file). The SHA-256 values below let you confirm the files are unchanged. Standard-library Python 3 only.

**Re-run:**
```
cd sim_v93
python3 run_all.py      # every scenario, seeds 1-50 (~5 min on 8 cores); writes results/raw_*.json
python3 analyze.py      # results/summary.json
python3 paired.py       # results/paired.json
python3 churn.py        # results/churn.json
python3 report.py       # this report
python3 trace.py "{}" 1 1126   # outside-world timeline of one job (any reproduction above)
```

**Seeds:** 1–50 for every Part 10 scenario and S17a–h; 1–20 for the 365-day Part 13 runs; 1–8 for the demand sweep. Printed per run in `results/run_log.txt`.

| File | Bytes | SHA-256 (first 16) | Role |
|---|---|---|---|
| analyze.py | 7,178 | `5d67193b8c487bbe` | Aggregates raw results |
| app.py | 12,885 | `f16564eecc79a126` | App base: DB, texting layer, consent, quiet hours, status machine |
| app_biz.py | 11,187 | `b518bcfc2b5572bf` | Business sign-up, identity chain, ROC checks, masked calls, Casey queue |
| app_flow.py | 30,374 | `4c88f805e6e1242e` | Cards, quotes, picks, confirm, charges, refunds, reconcile |
| app_intake.py | 22,616 | `4546b03a7c62eae5` | Inbound texts, opt-outs, safety, intake questions, cancel |
| checks.py | 43,414 | `5ca4ef9bfdfe3278` | Every Part 11 check, judged from outside-world records only |
| churn.py | 1,264 | `6cda7159a03d4541` | Churn by category (growth) |
| config.py | 14,635 | `6bcdfa4bb9ebfc97` | All Given/Assumed parameters, each labelled G or A |
| engine.py | 4,993 | `6477efb82c5094e8` | Discrete-event clock, timers, business-hours maths |
| metrics.py | 5,731 | `af5acc70f2694b4e` | Money by month, cash curve, workload, Part 9 metrics |
| paired.py | 1,488 | `63537bb397b682e0` | Same-seed differences |
| report.py | 51,028 | `99a08f72a2daa05e` | Builds this report |
| rerun_one.py | 584 | `fbf628e6892cb759` | Re-runs one scenario |
| run_all.py | 2,682 | `38fc9e5a49d8db49` | Runs everything on 8 processes |
| scenarios.py | 2,858 | `d268bbef3ba6ac8b` | Scenario definitions (Part 10 + S17a–h, S16 grid) |
| sim.py | 2,575 | `4c3951b04f84b6ed` | Builds one run from overrides + seed |
| trace.py | 1,632 | `595047bd3af8cec2` | Reproduction timelines |
| vendors.py | 18,649 | `15d9aab52f67e2e7` | Fake Twilio, Stripe, Claude, ROC/Ag/city sites, email, mail (with failure modes) |
| world.py | 38,817 | `b550c7b6cda7cfe3` | World truth, homeowner generation, outside-world records |
| world_biz.py | 10,618 | `9a11ad330aa6150e` | Businesses (join/leave/quote/confirm), fakes, gamers, Casey and backup |
| world_people.py | 14,727 | `4598614f5ce7c0f4` | Homeowner personas and their reactions |

**Scenario definitions (`scenarios.py`, in full):**
```python
"""Part 10 scenarios (+ my own S17 set) as config overrides. All values are Assumed unless the brief gives them (Given)."""
from config import POOL, CATS

GROWTH = {"m1": 1, "m12": 10, "m24": 16}                      # G: Part 10 #2
LICENSED = ["plumbing", "hvac_repair", "hvac_replace", "electrical"]   # G: approved hand-run test scope (licensed trades)
NEW_GROUPS = ["pool", "landscape", "cleaning", "pest", "boat", "detail", "handyman", "movers", "junk", "trashcan", "dogwaste",
              "window", "pwash", "solar"]

def thin_override():
    ov = {}
    half = NEW_GROUPS[::2]                                   # A: every other new category has a single business
    for g in half: ov[g] = 1
    ov["dogwaste"] = 0; ov["junk"] = 0                       # G
    return ov

SCEN = {
 "S01_normal":        {},
 "S02_growth":        {"days": 730, "growth": GROWTH},
 "S03_busy_summer":   {"requests_per_day": 16, "start_month": 7, "heat_wave": (20, 7, 3.0)},
 "S04_snowbird":      {"start_month": 1, "snowbird": True, "p_out_of_state_phone": 0.45},
 "S05_boat_season":   {"start_month": 5, "season": {"hvac_summer_mult": 3.0, "boat_season_mult": 4.0, "snowbird_clean_mult": 1.5}},
 "S06_vendor_chaos":  {"chaos": 3.0, "stripe_outage": (25, 4), "ai_outage": (35, 6)},
 "S07_twilio_suspend":{"tw": {"suspend_day": 30, "suspend_days": 30}},
 "S08_adversarial":   {"adversarial_mult": 2.0},
 "S09_casey_away":    {"casey": {"away": (20, 14)}},
 "S10_dispute_wave":  {"dispute_wave": (25, 5)},
 "S11a_collide_fwd":  {"requests_per_day": 16, "collision": "fwd"},
 "S11b_collide_rev":  {"requests_per_day": 16, "collision": "rev"},
 "S12_db_restore":    {"restore": {"day": 30, "lost_h": 3, "mode": "spec"}},
 "S13_crowded":       {"per_category_override": {g: 15 for g in POOL}},
 "S14_thin":          {"per_category_override": thin_override()},
 "S15_spanish20":     {"spanish_share_override": 0.20},
 # ---- S17: my choices
 "S17a_all_fixes":    {"fix": {k: True for k in ["code_scrub_before_ai", "no_photos_on_cards", "stable_list_numbers", "pick_idempotency_key",
                        "quiet_cutoff_2030", "spanish_optout", "consent_lenient", "pin_when_multi_job", "never_mind_cancel", "safety_followup",
                        "window_matches_promise", "handyman_disclosure", "casey_checklist", "emergency_indirect_keywords", "permit_question"]}},
 "S17b_approved_scope":{"approved_scope": True, "categories": LICENSED, "fees": "approved_kit"},
 "S17c_weekday_window":{"quote_window": "weekdays"},
 "S17d_roc_lists":    {"roc_mode": "lists"},
 "S17e_insurance_req":{"insurance_required": True},
 "S17f_rec_partial":  {"recurring_partial_refund": True},
 "S17g_roc_layout":   {"roc": {"forced_layout_day": 20}},
 "S17h_10_per_day":   {"requests_per_day": 10},
}
FEE_STRUCTS = ["base", "alt1", "alt2", "alt3", "alt4", "alt5"]
S50S = [0.10, 0.15, 0.20]
```

*End of report. Every figure is simulated; nothing here was observed in the real world.*
