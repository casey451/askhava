"""Builds Havasu_Sim_v93_Review.md from results/summary.json, paired.json, churn.json. Narrative text is written by hand
from the traces (trace.py) and the code; every table number is pulled from the result files."""
import json, os, hashlib, glob
D = os.path.dirname(os.path.abspath(__file__)); R = os.path.join(D, "results")
S = json.load(open(os.path.join(R, "summary.json"))); P = json.load(open(os.path.join(R, "paired.json")))
CH = json.load(open(os.path.join(R, "churn.json"))); SC = S["scen"]; CK = S["checks"]
def m(x, d=0): return f"{x:,.{d}f}" if x is not None else "n/a"
def usd(x): return ("-$" if x < 0 else "$") + f"{abs(x):,.0f}"
def pd(key, met="profit"):
    v = P[key][met]; return f"{v['mean_diff']:+,.{0 if met in ('profit',) else 2}f} ± {v['se']:,.{0 if met=='profit' else 2}f}"
def cell(s): return str(s).replace("|", "/").replace("\n", " ")
o = []; w = o.append
s1 = SC["S01_normal"]; g = SC["S02_growth"]
w("# Havasu Home Quotes: v9.3 simulation review\n")
w("Every number here comes from a **simulation**: no real person, business, text, call or payment was involved, and no real service was contacted. Labels: **[G]** = Given (stated in the brief), **[A]** = Assumed (a behaviour rate chosen by me or by the brief), **[M]** = Measured (sim output; mean over seeds 1–50 unless stated). A Measured number is only as good as the Assumed rates that drive it.\n")
w("## Verdict\n")
w(f"**Not ready to go live, and it should not go live before the legal gate.** The unit economics hold up [M, sim]: in the Normal scenario (5 requests/day [G], 60 days) profit is {usd(s1['profit'])} per 60 days (p10–p90 {usd(s1['profit_p10'])}–{usd(s1['profit_p90'])}) [M, sim], and cash never drops below {usd(s1['low_cash_min'])} [M, sim]. The app, though, breaks its own promises in ways an outside observer would notice. Homeowner names and addresses reach the AI and the job cards. A fake business can be switched on and handed a homeowner's details. A homeowner who says \"cancel\" or \"never mind\" can still end up with a business charged and holding their address. Masked calls sometimes ring the wrong homeowner. Some intake conversations stall forever. A Twilio suspension has no fallback. The **$20k–$50k a year** goal looks reachable at about {S['need']['20k_yr']:.1f} and {S['need']['50k_yr']:.1f} requests/day [M, sim]. **$20k–$50k a month is not reachable in this market**: profit levels off near $12k a month once every local business is already getting every card [M, sim].\n")
w("## Conflicts with your current decisions\n")
w(f"""1. **Goal: $20k–$50k a MONTH vs the brief's \"a YEAR\".** The brief (Parts 1 and 12) asks for $20,000 and $50,000 profit **a year**. Your current goal is $20k–$50k **a month**. I report both:
   - **Per year:** about **{S['need']['20k_yr']:.1f} requests/day** for $20k and **{S['need']['50k_yr']:.1f}/day** for $50k [M, sim; steady-state demand sweep].
   - **Per month:** not reached at any demand level tested (up to 100/day). Monthly profit levels off at about {usd(S['demand']['100']['steady_month_net'])} [M, sim]. A straight-line projection would say {S['need']['20k_mo'][1]:,.0f} and {S['need']['50k_mo'][1]:,.0f} requests/day, but that is meaningless: Lake Havasu City has about 57k residents [G], and the business pool saturates long before then.
   - **What a monthly goal would actually need:** more towns, higher fees, or a different product. Please confirm which goal is real; every recommendation below is sized to the yearly one.
2. **No homeowner name, phone or address may go into any AI tool, including Claude.** The plan breaks this in three places:
   - **Part 5.4 has the AI read the raw request.** People write \"This is Ruth Hernandez at 2060 Cherry Tree…\" in their first text. It also has the AI *scrub* the description, which by definition means the AI sees what it is scrubbing.
   - **The AI screens photos,** which can show house numbers.
   - **Part 11 only forbids this \"unless zero retention is in place\".** That directly conflicts with your rule, which has no such exception.
   - **Measured result:** the AI received a homeowner's name, phone or address {CK['S01_normal'][[k for k in CK['S01_normal'] if k.startswith('P3')][0]]['fails_per_run']:.1f} times per 60-day run in the Normal scenario [M, sim; check P3, every seed failed].
   - **Fix:** a plain-code scrubber runs *before* any AI call (names after \"this is / my name is\", street-number patterns, 7+ digits, emails), the AI only ever sees the scrubbed text, and photos are screened by code (OCR digits and faces blurred) or by Casey. With that fix (S17a), P3 dropped to {CK['S17a_all_fixes'][[k for k in CK['S17a_all_fixes'] if k.startswith('P3')][0]]['fails_per_run']:.1f} per run. The leftovers were things like \"John, [removed]\", so a code scrubber is still not perfect, and every scrubber miss is a rule breach.
3. **Legal and LLC work is deferred, with a hard gate before any business signs, any live charge or any real homeowner request.** These items in the plan would happen before that gate or need the entity first:
   - **A2P 10DLC brand and campaign registration** (Part 7; $19.50 one-time + $1.50/month [G]) needs a legal business name and EIN. Do it at the gate, not before.
   - **Stripe account and customer creation at sign-up** (Part 5.5) creates a payment relationship with a business, so it sits behind the gate.
   - **Business sign-ups, identity checks and mailed letters** (Parts 3 #13 and 8.2) contact real businesses, so they sit behind the gate.
   - **The city business license and terms of service** need an entity.
   - **Pre-gate work** should be limited to this simulation, code, the scrubber, test numbers and fake data.
4. **No hard calendar dates.** The brief carries dated labels (the date on the change list, a permit report month) as source references, not plan dates. This report uses only phases (pre-gate, hand-run test, launch, growth month N) and relative days (day 30 of a run).
5. **Scope changes from the approved hand-run test (your choices, noted neutrally).** The approved hand-run test was Lake Havasu City only, licensed trades only, fees of $45/$59/$149, and no automated line. v9.3 + Oct 26 adds 25 categories including unlicensed and recurring ones, $25/$75/$199 tiers, a fully automated text line, and neighbouring towns later. Risk judgement:
   - **More businesses and more matching:** the approved scope (S17b) keeps only {SC['S17b_approved_scope']['biz_kept']:.0f} businesses vs {s1['biz_kept']:.0f} [M, sim], gets {SC['S17b_approved_scope']['quotes_per_job']:.2f} quotes per job vs {s1['quotes_per_job']:.2f}, and shows {SC['S17b_approved_scope']['picks_confirmed_share']:.2f} vs {s1['picks_confirmed_share']:.2f} confirmed picks. So the wider scope is what makes the matching work.
   - **More exposure:** the wider scope also brings the handyman-exemption, permit, gas and pest-license matching failures (X1/X3/X4), more fake-business openings (unlicensed categories have no ROC identity chain), and more AI handling of personal data.
   - **Profit is close:** S17b profit {usd(SC['S17b_approved_scope']['profit'])} vs {usd(s1['profit'])} per 60 days [M, sim; paired diff {pd('S17b_approved_scope - S01_normal')}].
   - **My read:** the automated line is the biggest risk increase, because every bug below exists only because software, not Casey, is making the move. A hand-run phase on the wider category list would catch most of them cheaply.
6. **Ask Hava (askhava.com)** is modelled with **zero** demand [G] and is not part of any number here.

### Licensing check (attorney must confirm before the gate)
- **Handyman exemption, A.R.S. §32-1121(A)(14):**
  - **The $1,000 limit** covers the *total* job, labour plus materials [G]. The 2025 bill text (HB2120, linked in the brief) is the source; an attorney should confirm the current limit before launch. The app refuses handyman quotes of $1,000 or more, using the top of a range, but only in the handyman and licensed categories. A handyman-type business quoting $2,408 on pressure washing was shown and charged $199 (seed 16, #1216). That is legal only if the work has no sealing or painting.
  - **The permit rule:** a job needing a permit is never exempt. The app never asks whether a permit is needed (the `permit_question` fix is off by default), so water-heater replacements, which need a permit in most cases, reached handymen (X1/X3).
  - **No splitting:** the app has no rule against repeat cards for the same address and category inside a short window, and it needs one.
  - **The \"not a licensed contractor\" disclosure:** the exemption requires handymen to say so in advertising and contracts. Their quote card shows no such line (the `handyman_disclosure` fix is off by default). Add it to every handyman quote card and to the business terms.
- **Fee that rises with quote size:** a $25/$75/$199 fee tied to the chosen quote looks like a *share of the contract price*, i.e. a commission, rather than a flat referral or listing fee.
  - Combined with the app *refusing* quotes, reading and reformatting quotes, and controlling the pick-and-confirm step, that strengthens an argument that the platform is acting \"by or through others\" under §32-1101's contractor definition, against the \"we're just compiling the information\" positioning.
  - Lower-risk shapes: a flat per-introduction fee (alt3, but it costs profit, see Price reaction) or a per-category flat fee.
  - Either way, an attorney should review the tier structure, the quote refusal rule, and the wording \"we gather quotes; you choose\" before the gate.
- **Pest/weed control:** routing \"weed control\" only to Dept. of Agriculture licensees held (X2: 0 failures in Normal).
- **Pool repair gas heaters and gas appliances:** they reached a handyman in a few runs (X3 below).
""")
w("## 1. Could you run code?\n")
log = open(os.path.join(R, "run_log.txt")).read().strip().splitlines()
w(f"**Yes.** A standard-library Python 3 discrete-event simulator was built in `sim_v93/`, with fake Twilio (SMS, MMS, Verify, voice), Stripe, Claude, ROC, Dept. of Agriculture and city-list sites, email and mail. Every scenario was run:\n- **Part 10 scenarios S01–S15 and S17a–h:** seeds **1–50** each, 60 days each.\n- **S02 Growth:** seeds 1–50, 730 days (24 months).\n- **S16:** 6 fee structures × s50 0.10/0.15/0.20 plus the first-90-days variant, seeds 1–50.\n- **Long runs for Part 13:** 365 days, seeds 1–20.\n- **Demand sweep:** 2–100 requests/day, 120 days, seeds 1–8.\n\nRun log: `{log[-1] if log else ''}`. Checks are judged only from outside-world records: the vendor logs (texts actually delivered, Stripe charges, connected calls, AI inputs) and each simulated person's own memory. They never use app status labels.\n")
w("## 2. Summary: top 5\n")
def ck(sc, pre): return CK[sc][[k for k in CK[sc] if k.startswith(pre)][0]]
w(f"""1. **Homeowner personal data reaches the AI and the job cards** [M, sim]. P3: {ck('S01_normal','P3')['fails_per_run']:.0f} AI inputs per run carried a name, phone or address. P2: {ck('S01_normal','P2')['fails_per_run']:.0f} job cards per run carried a name or contact phrase. Both failed in 50/50 runs. This breaks your no-PII-to-AI rule and the card promise.
2. **Fake or unverified businesses can receive homeowner details** [M, sim]: P1 failed in {ck('S01_normal','P1')['runs_failing']}/50 Normal runs and {ck('S08_adversarial','P1')['runs_failing']}/50 Adversarial runs. The cause: when the ROC result is \"unknown\" or the business is in an unlicensed category, Casey's approve-signup screen can activate it with no completed identity chain.
3. **Cancels don't reliably stop money or details** [M, sim]. M1 (charge after cancel or on a dead job): {ck('S01_normal','M1')['fails_per_run']:.2f}/run, {ck('S01_normal','M1')['runs_failing']}/50 runs. H6: \"please cancel\" / \"never mind\" not honoured, {ck('S01_normal','H6')['fails_per_run']:.2f}/run. The app closes the wrong job when a homeowner has several, and turns \"Never mind…\" into a new request.
4. **The economics work for the YEAR goal only** [M, sim]: about {S['need']['20k_yr']:.1f}/day for $20k/yr and {S['need']['50k_yr']:.1f}/day for $50k/yr. In the growth scenario the $20k/yr pace first arrives in month {S['growth_month'].get('20k_yr')} (median); the $50k/yr pace is {('month '+str(S['growth_month'].get('50k_yr'))) if S['growth_month'].get('50k_yr') else 'not reached in the 24-month mean'}. Business churn is high: about {g['biz_joined']:.0f} joined vs {g['biz_kept']:.0f} kept over 24 months, because $25 on a ~$300 repair crosses the assumed fee-too-high line.
5. **A 30-day Twilio suspension costs {usd(S['suspension']['profit_loss'])} and +{S['suspension']['casey_extra_min_week']:.0f} Casey minutes/week (~{S['suspension']['casey_extra_hours_total']:.0f} h)** per 60-day run [M, sim]. There is no fallback channel: {SC['S07_twilio_suspend']['broken_promises']:.0f} broken promises per run, and new requests go unanswered (H4).
""")
# ---------- problems
w("## 3. Problems found, ranked by severity\n")
w("Reproductions use `python3 trace.py \"<overrides>\" SEED JOB` (overrides `{}` = Normal). Days are run-relative; times are simulated clock times. Rates are failures per 60-day run [M, sim].\n")
w("| # | Severity | Problem | Scenario, seed, day | What happened, step by step | Suggested fix |\n|---|---|---|---|---|---|")
def ex(sc, pre): e = ck(sc, pre)["example"]; return f"seed {e[0]}: {cell(e[1])[:150]}" if e else "—"
rows = [
("Critical", f"Homeowner name, address and phone sent to the AI (P3: {ck('S01_normal','P3')['fails_per_run']:.0f}/run, 50/50 runs)", "S01, seed 1, day 2 11:51, job #1056", "1) The homeowner's first text: \"boat outboard motor won't start This is Ruth Hernandez at 2060 Cherry Tree Blvd, call me 206-500-0015.\" 2) The app sends the raw text to the AI to read the request (Part 5.4). 3) The fake Claude input log contains the name, the address and the phone.", "Plain-code scrubber before *every* AI call (\"this is / my name is\" + capitalised words, street-number patterns, 7+ digits, emails). The AI only sees scrubbed text. Photos are screened by code (OCR for digits, face blur) or by Casey. Remove the Part 11 \"unless zero retention\" exception."),
("Critical", f"Names and contact phrases on job cards (P2: {ck('S01_normal','P2')['fails_per_run']:.0f}/run, 50/50)", "S01, seed 1, job #1056, card to B0247", "1) The same text is scrubbed: the full name, address and phone are removed. 2) \"(ask for Ruth)\" survives the AI scrub and the code pass. 3) The delivered card reads \"…boat outboard motor won't start (ask for Ruth)\".", "Code pass: drop any sentence containing \"ask for\", \"call me\", \"I'm\", \"this is\" or a capitalised given name from a name list. Keep the weekly 10-card spot-check."),
("Critical", f"Fake or unverified businesses activated and given details (P1: {ck('S01_normal','P1')['runs_failing']}/50 Normal runs, {ck('S08_adversarial','P1')['fails_per_run']:.1f}/run Adversarial; B3)", "S01, seed 1, business F0380 → job #1221, day 36 17:57", "1) F0380 (fake) signs up in an unlicensed category. 2) The only identity step is a code sent to *the phone F0380 itself typed in*, which it passes, then a manual approval by Casey. 3) It quotes and is picked on job #1221. 4) It confirms and pays, and gets the homeowner's first name and address. Related case, seed 1 B0019: the ROC result was 'unknown', the sign-up was held, Casey's approval activated it with no identity code at all, and the licence was never read back (B2).", "Never activate on approval alone. Licensed: require a code to the phone or email on the ROC record, falling through voice and letter. Unlicensed: require a city-list name match plus a mailed letter code, or an insurance certificate. S17e (insurance required) cut active fakes from .36 to .20 per run."),
("High", f"Charge and details after the homeowner cancelled (M1 {ck('S01_normal','M1')['fails_per_run']:.2f}/run, {ck('S01_normal','M1')['runs_failing']}/50; M6; H6 {ck('S01_normal','H6')['fails_per_run']:.2f}/run)", "S01, seed 1, homeowner +19285000063, job #1126, days 14–17", "1) The homeowner has two requests open (#1111 yard, #1112 doors). 2) Day 15 20:15 \"please cancel, I found someone\": the app closes only one job. 3) Day 16 15:47 \"Never mind, neighbor fixed it\" is not treated as a cancel (the never-mind rule is off by default). Its words match no live job, so the app opens a *new* job #1126, carrying the door text, and sends cards to B0303 and B0304. 4) Day 17 08:45 a pick is recorded. 5) Day 17 09:52 B0303 pays $25 and receives \"James, James, 1082 Kiowa Blvd\". The money is kept (M6).", "When there is more than one open job, ask \"Cancel which? 1) yard 2) doors 3) both\". Treat never-mind / no-longer-need / ya-no as cancel intent. Never open a new job from a cancel-like message. Refund anything charged after a cancel."),
("High", f"Masked calls ring the wrong homeowner; the PIN is ignored (C1 {ck('S01_normal','C1')['fails_per_run']:.1f}/run; C2 {ck('S01_normal','C2')['fails_per_run']:.1f}/run)", "S01, seed 1, job #1092, day 9 09:53 to day 10 17:14", "1) Sun Hvac 34 (B0034) has two connected jobs. 2) It calls the masked line four times (one call with PIN 7548). 3) With more than one job the router picks the *most recently released* job and ignores the PIN, so every call rings +13125000045, another homeowner. 4) Day 12: Ruth is told \"Sun Hvac 34 didn't call\". The business had paid $25.", "With more than one job, always ask for the PIN and route only by PIN (the `pin_when_multi_job` fix). After 2 bad PINs, read out the first name and street before connecting. Or use one proxy number per job."),
("High", f"Twilio suspension has no fallback (S07: H4 {ck('S07_twilio_suspend','H4')['fails_per_run']:.0f}/run, N1 {ck('S07_twilio_suspend','N1')['fails_per_run']:.0f}/run, {SC['S07_twilio_suspend']['broken_promises']:.0f} broken promises/run)", "S07, any seed, days 30–60", "1) Day 30: the number is suspended and every SMS fails. 2) New requests get no reply, picks can't be made by text, and businesses miss picks. 3) Jobs pile up in the 'Needs you' list; Casey's time rises by about 124 min/week.", "A backup number on a second provider, registered at the gate. Email and web-chat fallback for anyone with an email or a live web session. A status line on the website. Strict 10DLC compliance (the T1/T2 fixes below) to avoid suspension in the first place."),
("High", f"Intake jobs stall forever (N1 {ck('S01_normal','N1')['fails_per_run']:.0f}/run, 50/50)", "S01, seed 1, job #1086, from day 8 08:02", "1) The app asks \"what's the street address and your first name?\" 2) The homeowner never answers. 3) There is no reminder and no close; the job is still in Intake at day 60.", "Reminder at 24 h, close at 72 h (\"Text us any time to restart\"). Use the same timeout pattern for every waiting state."),
("High", f"Unlicensed handyman gets permit or gas work (X1 {ck('S01_normal','X1')['fails_per_run']:.0f}/run, 49/50; X3 {ck('S01_normal','X3')['fails_per_run']:.2f}/run)", "S01, seed 1, job #1167; seed 7, job #1057", "1) An electric water heater job (#1167) is classed plumbing / water_heater_elec. 2) Water heater replacement normally needs a permit, but the app never asks about permits. 3) The card goes to handyman B0303, who holds no licence. Seed 7: a gas water heater (#1057) card goes to handyman B0306, against the no-gas rule.", "Permit-type job list (water heaters, panels, new circuits, re-roofs and the like) that is never sent to handymen. A gas keyword list overrides the AI. The `permit_question` and `handyman_disclosure` fixes on by default."),
("High", f"First text lacks disclosures; quiet hours broken for out-of-state phones (T1 {ck('S01_normal','T1')['fails_per_run']:.0f}/run; T2 {ck('S01_normal','T2')['fails_per_run']:.0f}/run)", "S01, seed 1: +19285000008 (T1); day 2 08:00 to +14155000006, local 07:00 (T2)", "T1: 1) A homeowner's first message is 1–2 words. 2) The app replies \"Hi, this is Havasu Home Quotes. Tell us a bit more…\" with no STOP/HELP, rates, frequency, terms or 'business pays us' wording. 3) That is the number's first text. T2: 1) Quiet hours are applied in Arizona time. 2) The 08:00 Arizona batch reached a 415 phone at 07:00 its local time.", "Send the full disclosure text as the first message in every path. Apply quiet hours in the recipient's area-code zone, using the stricter of Arizona time and that zone."),
("Medium", f"Late safety text (S1 {ck('S01_normal','S1')['fails_per_run']:.1f}/run, {ck('S01_normal','S1')['runs_failing']}/50 runs)", "S01, seed 1, job #1081, day 6 17:16", "1) \"breaker panel is sparking breaker keeps tripping\". 2) The keyword list has \"sparks\" and the match is word-prefix, so \"sparking\" is missed. 3) The consent text goes first. 4) The safety text is sent at 17:24, after the homeowner replies OK and the AI flags it: 8 minutes.", "Match word stems (spark, smok, burn, flood, gas, hot, buzz, melt, plus Spanish equivalents). Send the safety text before the consent text."),
("Medium", f"Restore re-sends texts (R3 {ck('S12_db_restore','R3')['fails_per_run']:.1f}/run, {ck('S12_db_restore','R3')['runs_failing']}/50 runs)", "S12, seed 3, job #1182, day 30", "1) Lost window: day 30 11:00–14:00. 2) At 12:49 the business gets \"Job #1182 is no longer active\". 3) After the 14:00 restore, the replay re-runs the pick and sends the same text again.", "During replay, check Twilio's sent log by (job, template, recipient) before any send. Message IDs should be derived the same way."),
("Medium", f"Second free first win through a second name (M3 {ck('S01_normal','M3')['fails_per_run']:.2f}/run)", "S01, seed 1, B0340 / B0340D (jobs #1080, #1220)", "1) The business signs up again under a DBA with another phone. 2) It is unlicensed, so the entity key is its *name*. 3) The name differs, so it gets a second free introduction.", "Key free wins on ROC number OR city-list entity OR card/bank fingerprint, checked when the free win is used."),
("Medium", f"Books differ from Stripe (M4 {ck('S01_normal','M4')['fails_per_run']:.2f}/run)", "S01, seed 8 (books $2,694 vs Stripe $2,719)", "1) Stripe records a successful charge that the app's ledger never booked. 2) The nightly reconcile raises an 'unknown charge' alert but posts nothing and refunds nothing.", "Reconcile from Stripe's event list. Auto-book or auto-refund the charge (refund if the job is closed), and keep the alert."),
("Medium", f"Losing businesses not told (H7 {ck('S01_normal','H7')['fails_per_run']:.0f}/run)", "S01, seed 1, job #1054, B0134", "1) 'No longer active' goes out once, at pick time, only to businesses that had quoted by then. 2) Quotes are still accepted after the pick (Awaiting confirm / Payment pending). 3) Anyone who quotes later, like B0134, is never told.", "Stop accepting quotes once a pick is confirmed, or send 'no longer active' to every quoter whenever the job ends."),
("Medium", f"Update texts closer than 3 hours (H2 {ck('S01_normal','H2')['fails_per_run']:.0f}/run); list numbers move", "S01, seed 1, job #1097, day 10", "1) An update is held overnight and delivered at 08:00. 2) The 3-hour gap is counted from when the app queued it, so the next update goes at 10:24 (failed) and 10:26 (delivered): 2.4 h after the last one the homeowner received. 3) The list is re-sorted by price, so 'Canyon Pool 77' moves from #1 to #2 between texts, and '1 YES' could pick a different business than the homeowner meant.", "Count the gap from delivery time. Keep the list in arrival order (the `stable_list_numbers` fix) or confirm the business name before the pick."),
("Medium", f"Business texts and questions not answered (B1 {ck('S01_normal','B1')['fails_per_run']:.1f}/run; B4 {ck('S01_normal','B4')['fails_per_run']:.2f}/run)", "S01, seed 1, day 52 11:57 (B1); seed 7, F0380 day 33 16:07 (B4)", "1) \"Do you take handymen? want work\" doesn't match the business phrase list, so no sign-up link is sent. 2) \"picked? what do I do\" goes to the Needs-you list instead of an automatic reply.", "Widen the business-intent phrases, with an AI second opinion on scrubbed text. Auto-reply to any free-text message from a business with a live pick."),
("Low", "Details text repeats the first name", "S01, seed 1, job #1092 (\"Ruth, Ruth, 2918 McCulloch Blvd\")", "The details template prints the first name twice.", "Fix the template."),
("Low", f"Handyman quote ≥$1,000 on an unlicensed category shown (X4 {ck('S01_normal','X4')['fails_per_run']:.2f}/run)", "S01, seed 16, job #1216", "1) A pressure-wash job (no licence needed). 2) River Handyman 302 quotes $2,408. 3) The refusal rule applies only to handyman or licensed categories, so it is shown, picked, and $199 is charged.", "Legal if the work includes no sealing or painting (R-34). Ask the attorney whether handyman-type businesses should be capped across all categories."),
]
for i, r in enumerate(rows, 1): w(f"| {i} | " + " | ".join(cell(c) for c in r) + " |")
# ---------- checks
w("\n## 4. Checks table (Part 11)\n")
w("**How to read this.** Each check is judged only from outside-world records: texts actually delivered, Stripe objects, connected calls, AI input logs, and what simulated people did and knew. **PASS** means zero failures in all 24 scenarios × 50 seeds. Columns:\n- **Opps:** opportunities per run in S01.\n- **S01 f/run:** S01 failures per run.\n- **runs:** S01 runs with a failure, out of 50.\n- **Scen. failing:** scenarios with at least one failure, out of 24.\n- **Worst:** the scenario with the most failures per run.\n\nAll [M, sim].\n")
w("| Check | Result | Opps (S01) | S01 f/run | runs /50 | Scen. failing | Worst | Example (seed: what the outside world saw) |\n|---|---|---|---|---|---|---|---|")
allk = []
for sc in CK:
    for k in CK[sc]:
        if k not in allk: allk.append(k)
npass = nfail = 0
for k in allk:
    have = [(sc, CK[sc][k]) for sc in CK if k in CK[sc]]
    nf = sum(1 for sc, v in have if v["fails_total"] > 0)
    worst = max(have, key=lambda x: x[1]["fails_per_run"])
    base = CK["S01_normal"].get(k) or have[0][1]
    exs = base["example"] or worst[1]["example"]
    res = "PASS" if nf == 0 else "FAIL"; npass += nf == 0; nfail += nf > 0
    w(f"| {cell(k)} | **{res}** | {base['opps']/50:,.0f} | {base['fails_per_run']:.2f} | {base['runs_failing']} | {nf}/{len(have)} | {worst[0].split('_')[0]} {worst[1]['fails_per_run']:.2f} | {('seed '+str(exs[0])+': '+cell(exs[1])[:120]) if exs else '—'} |")
w(f"\n**Totals: {npass} PASS, {nfail} FAIL out of {len(allk)} judged checks** [M, sim]. R1 and R3 are judged only in S12; R2 equals M1 there. K1 (Casey's minutes per week) is a measurement, reported in section 7. Low opportunity counts (B6, B7, X2) mean a PASS is weak evidence. With every fix on (S17a), the remaining failures per run were: " +
  ", ".join(f"{k.split()[0]} {v['fails_per_run']:.2f}" for k, v in CK["S17a_all_fixes"].items() if v["fails_per_run"] >= 0.5) + ". T1 got *worse* under the fixes (the lenient-consent fix sends a short first text without disclosures), so don't ship that fix.\n")
w("**Per-scenario failure totals** (sum of failures per run across all checks; hot spots) [M, sim]:\n")
w("| Scenario | total f/run | top 3 checks |\n|---|---|---|")
for sc in CK:
    tot = sum(v["fails_per_run"] for v in CK[sc].values()); top = sorted(CK[sc].items(), key=lambda x: -x[1]["fails_per_run"])[:3]
    w(f"| {sc} | {tot:.0f} | " + ", ".join(f"{k.split()[0]} {v['fails_per_run']:.1f}" for k, v in top) + " |")
# ---------- money
w("\n## 5. Money (Part 12)\n")
w("""**Given [G]:**
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
""")
cols = [("requests", 0), ("picks", 0), ("paid_intros", 0), ("free_wins", 0), ("n_refunds", 1), ("revenue", 0), ("refunds", 0), ("stripe_fees", 0), ("dispute_fees", 0), ("twilio_sms", 0), ("twilio_verify", 0), ("twilio_voice", 0), ("claude", 0), ("letters", 0), ("email", 0), ("fixed", 0), ("net", 0), ("cost_per_request", 2), ("avg_fee", 0)]
hdr = "| Month | " + " | ".join(c for c, _ in cols) + " |\n|" + "---|" * (len(cols) + 1)
w("### 5.1 Normal (S01), by month [M, sim; mean of seeds 1–50]\n"); w(hdr)
for i, mo in enumerate(s1["months"], 1): w(f"| {i} | " + " | ".join(m(mo.get(c, 0), d) for c, d in cols) + " |")
w("\n`twilio_sms` includes MMS and the number and 10DLC fees [G]. `email` costs $0 at this volume [A, free tier]. `cost_per_request` = vendor + fixed costs divided by requests.\n")
w("### 5.2 Growth (S02, 24 months), by month [M, sim; mean of seeds 1–50]\n")
w("Demand ramps from 1/day to 10/day by month 12 and 16/day by month 24 [G]. Businesses join over time [A]. Month 24 is normalised to 30 days.\n"); w(hdr)
for i, mo in enumerate(g["months"], 1): w(f"| {i} | " + " | ".join(m(mo.get(c, 0), d) for c, d in cols) + " |")
gm = S["growth_month"]; gps = S["growth_month_per_seed"]
reach50 = [x for x in gps["50k_yr"] if x]
w(f"""
**Growth milestones** [M, sim]:
- **$20k/yr pace (≥ $1,667/month):** first reached in month **{gm.get('20k_yr')}** of the mean path. Per seed: median month {sorted(gps['20k_yr'])[25]}, range {min(gps['20k_yr'])}–{max(gps['20k_yr'])}.
- **$50k/yr pace (≥ $4,167/month):** the mean path {('reaches it in month '+str(gm.get('50k_yr'))) if gm.get('50k_yr') else 'never reaches it in 24 months'}. {len(reach50)}/50 seeds touch it in at least one month (median month {sorted(reach50)[len(reach50)//2] if reach50 else 'n/a'}), but few stay there.
- **$20k or $50k a MONTH:** never reached on any seed.
- **24-month total profit:** {usd(g['profit'])} (p10–p90 {usd(g['profit_p10'])}–{usd(g['profit_p90'])}).
- **Businesses:** {g['biz_joined']:.0f} joined, {g['biz_kept']:.0f} active at the end.
- **Late decline:** monthly net falls in months 21–23 as churn outpaces new sign-ups. See the churn table in section 6.
""")
w("### 5.3 Requests per day needed (demand sweep; 120-day runs, seeds 1–8; steady state = months 3–4) [M, sim]\n")
w("| Requests/day | Steady monthly net | Annualised | Businesses kept |\n|---|---|---|---|")
for k in sorted(S["demand"], key=lambda x: int(x)):
    v = S["demand"][k]; w(f"| {k} | {usd(v['steady_month_net'])} | {usd(v['annual'])} | {v['biz_kept']:.0f} |")
w(f"""
- **$20,000 a year:** about **{S['need']['20k_yr']:.1f} requests/day** (interpolated).
- **$50,000 a year:** about **{S['need']['50k_yr']:.1f} requests/day**.
- **$20,000 or $50,000 a MONTH:** not reached at any tested level. Monthly profit levels off near {usd(S['demand']['100']['steady_month_net'])}, because every local business already gets every card and businesses leave faster at high volume. A straight-line projection ({S['need']['20k_mo'][1]:,.0f} and {S['need']['50k_mo'][1]:,.0f}/day) is not meaningful.
""")
w("### 5.4 Every scenario, per 60-day run [M, sim; mean of seeds 1–50]\n")
w("| Scenario | Profit | p10 | p90 | Paid intros | Free wins | Refunds n | Refunds $ | Broken promises | Casey min/wk | Low cash mean | Low cash worst (seed) |\n|---|---|---|---|---|---|---|---|---|---|---|---|")
for sc, v in SC.items():
    t = v["total"]; w(f"| {sc} | {usd(v['profit'])} | {usd(v['profit_p10'])} | {usd(v['profit_p90'])} | {t['paid_intros']:.0f} | {t['free_wins']:.0f} | {v['refunds_n']:.1f} | {usd(t['refunds'])} | {v['broken_promises']:.1f} | {v['casey_min_week']:.0f} | {usd(v['low_cash'])} | {usd(v['low_cash_min'])} ({v['low_cash_seed']}) |")
lowest = min(SC.items(), key=lambda x: x[1]["low_cash_min"])
w(f"""
- **'Broken promises'** counts outside-world failures of the four homeowner promises: first-quote text (H1), the 9 p.m. promise (H3), a reply within 24 h (H4) and cancel honoured (H6).
- **Lowest cash point** across every scenario and seed: **{usd(lowest[1]['low_cash_min'])}** ({lowest[0]}, seed {lowest[1]['low_cash_seed']}), measured against $1,000 operating cash. The $500 Stripe reserve is held separately and never needed [A]. Cash never goes negative in any run [M, sim], because early free wins cost only vendor fees.
- **30-day texting suspension (S07 vs S01, same seeds):**
  - profit −{usd(S['suspension']['profit_loss'])} per 60-day run;
  - Casey +{S['suspension']['casey_extra_min_week']:.0f} min/week, about **{S['suspension']['casey_extra_hours_total']:.0f} extra hours** over the run;
  - broken promises +{SC['S07_twilio_suspend']['broken_promises']-s1['broken_promises']:.0f}.

  [M, sim]
""")
# ---------- price reaction
F = S["fees"]
w("\n## 6. Price reaction (Part 9)\n")
w("""### Rules used (all [A] unless marked)
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
""")
w("### Every fee structure × s50 (Scenario 16; 60 days; seeds 1–50) [M, sim]\n")
w("**Structures:** base = $25/$75/$199 [G]; alt1 = $19/$59/$199; alt2 = $25/$75/$300; alt3 = flat $25; alt4 = 5% (min $15, max $250); alt5 = base with recurring counted at 4 months; first90 = base with free wins only in the first 90 days. Columns:\n- **Conf.:** share of picks confirmed.\n- **Joined/kept:** businesses joined and still active at the end.\n- **Q/job:** quotes per job.\n- **Pick:** homeowner pick rate.\n- **Avg fee:** average fee per paid intro.\n- **Gaming:** fee avoided / caught.\n")
w("| Structure | s50 | Profit | p10–p90 | Conf. | Joined / kept | Q/job | Pick | Avg fee | Casey min/wk | Gaming: fee avoided / caught |\n|---|---|---|---|---|---|---|---|---|---|---|")
for fs in ["base", "alt1", "alt2", "alt3", "alt4", "alt5", "first90"]:
    for s50 in ["s10", "s15", "s20"]:
        k = f"S16_{fs}_{s50}"
        if k not in F: continue
        v = F[k]; af = v["total"]["revenue"] / max(1e-9, v["total"]["paid_intros"])
        w(f"| {fs} | 0.{s50[1:]} | {usd(v['profit'])} | {usd(v['profit_p10'])}–{usd(v['profit_p90'])} | {v['picks_confirmed_share']:.2f} | {v['biz_joined']:.0f} / {v['biz_kept']:.0f} | {v['quotes_per_job']:.2f} | {v['ho_pick_rate']:.2f} | {usd(af)} | {v['casey_min_week']:.0f} | {usd(v['fee_avoided'])} / {v['gamed_caught']:.2f} of {v['n_gamed']:.2f} |")
w(f"""
**What this says** [M, sim]:
- **Most profit at every s50:** alt5 (recurring at 4 months), then alt2 ≈ base.
- **Flat $25 (alt3)** roughly halves profit.
- **alt1 and alt4** keep more businesses and lift confirms, quotes per job and pick rate, but lose money overall.
- **Sensitivity:** moving s50 from 0.15 to 0.10 costs the base structure about {usd(F['S16_base_s15']['profit']-F['S16_base_s10']['profit'])} per 60 days. That makes the hand-run test's measured confirm rate the single most valuable number to collect.
- **Gaming** is small in money terms (one gamer per run, tens of dollars avoided), and the portal flag plus PROBLEM reply catch only a minority. The $500 and $1,500 cliffs invite shading.
""")
w("### Churn by category (Growth, 24 months, seeds 1–50) [M, sim]\n")
w("| Category group | Activated/run | Left/run | Share left | Leavers with 0 wins |\n|---|---|---|---|---|")
for gname, v in sorted(CH.items(), key=lambda x: -x[1]["left_share"]):
    w(f"| {gname} | {v['activated_per_run']:.1f} | {v['left_per_run']:.1f} | {v['left_share']:.0%} | {v['left_with_0_wins_share']:.0%} |")
w("\n**Licensed repair trades (HVAC, plumbing, electrical, handyman) lose about 72–79% of the businesses that join within 24 months**, almost all of them *after* winning [M, sim]. The cause is the Assumed leave rule: a $25 fee on a ~$300 repair is about 8% of the job, above the s50/2 = 7.5% strike line, so those trades pile up strikes. Recurring categories (landscape, cleaning, pool) churn least; most of their leavers never won. If real businesses tolerate $25 on small repairs better than this rule assumes, churn is overstated. The hand-run test must measure this.\n")
# ---------- workload
w("## 7. Casey's workload (minutes per week by task) [M, sim]\n")
w("Task minutes are [A] (config.py `casey.minutes`). Casey works the 'Needs you' list only in **two 30-minute windows a day** around his shifts [A]: two weeks of day shifts (06:15 and 19:45), then two weeks of nights (12:00 and 17:00). That caps him at 420 min/week. Items wait while he is on shift. The **backup** column is the backup person's minutes per week during S09's two-week absence.\n")
tasks = sorted({t for v in SC.values() for t in v["casey_tasks"]})
show = ["S01_normal", "S02_growth", "S03_busy_summer", "S06_vendor_chaos", "S07_twilio_suspend", "S08_adversarial", "S09_casey_away", "S13_crowded", "S17a_all_fixes", "S17h_10_per_day"]
w("| Task | " + " | ".join(x.split('_')[0] for x in show) + " | S09 backup (away) |\n|---|" + "---|" * (len(show) + 1))
for t in tasks:
    w(f"| {t} | " + " | ".join(f"{SC[x]['casey_tasks'].get(t, 0):.0f}" for x in show) + f" | {SC['S09_casey_away']['backup_tasks'].get(t, 0):.0f} |")
w("| **Total** | " + " | ".join(f"**{SC[x]['casey_min_week']:.0f}**" for x in show) + f" | **{SC['S09_casey_away']['backup_min_week_away']:.0f}** |")
w(f"""
- **On shift vs away:**
  - Normal load is about {s1['casey_min_week']:.0f} min/week, roughly 1.3 hours.
  - The biggest items are pause reviews, the weekly spot-check and the ledger check.
  - When Casey is away (S09) the backup needs about {SC['S09_casey_away']['backup_min_week_away']:.0f} min/week, mostly reviewing paused businesses.
  - Casey's own load averages {SC['S09_casey_away']['casey_min_week']:.0f} min/week over S09 because of the catch-up after he returns.
- **Load spikes:**
  - a texting suspension (S07): {SC['S07_twilio_suspend']['casey_min_week']:.0f} min/week;
  - a crowded market (S13): {SC['S13_crowded']['casey_min_week']:.0f};
  - all fixes on (S17a): {SC['S17a_all_fixes']['casey_min_week']:.0f} min/week, because the strict identity fixes push many launch sign-ups to manual approval. Make that one fix cheaper (for example, batch approvals) before enabling it.
- **24-month growth:** {g['casey_min_week']:.0f} min/week average.
""")
# ---------- vendor behaviour
w("## 8. Vendor behaviour [M, sim; per 60-day run]\n")
vs = ["S01_normal", "S06_vendor_chaos", "S07_twilio_suspend", "S10_dispute_wave", "S17g_roc_layout", "S02_growth"]
keys = ["sms", "sms_failed", "verify", "calls", "claude", "emails", "letters", "charges", "charges_failed", "disputes", "refunds"]
w("| Scenario | " + " | ".join(keys) + " |\n|---|" + "---|" * len(keys))
for x in vs: w(f"| {x} | " + " | ".join(f"{SC[x]['vendor_calls'].get(k, 0):,.1f}" for k in keys) + " |")
def ckf(sc, pre): return ck(sc, pre)["fails_per_run"]
w(f"""
- **Twilio:**
  - Normal: {SC['S01_normal']['vendor_calls']['sms_failed']/SC['S01_normal']['vendor_calls']['sms']:.1%} of texts fail [A rate, M count].
  - Chaos (S06, all failure rates ×3 [G]): {SC['S06_vendor_chaos']['vendor_calls']['sms_failed']/SC['S06_vendor_chaos']['vendor_calls']['sms']:.0%} fail. First-quote texts then go missing (H1 {ckf('S06_vendor_chaos','H1'):.1f}/run vs {ckf('S01_normal','H1'):.2f}), and identity codes fail more (B2 {ckf('S06_vendor_chaos','B2'):.1f}/run).
  - The app retries important texts once and has no other channel.
  - Suspension (S07) is the single worst vendor event: N1 {ckf('S07_twilio_suspend','N1'):.0f}/run and H4 {ckf('S07_twilio_suspend','H4'):.0f}/run.
- **Stripe:**
  - Declines: {SC['S01_normal']['vendor_calls']['charges_failed']:.1f} per run in Normal and {SC['S06_vendor_chaos']['vendor_calls']['charges_failed']:.1f} in Chaos. The 1-business-hour card window works.
  - The 4-hour outage in S06 released picks as designed.
  - Books-vs-Stripe mismatches (M4) come from charges the app never booked; the nightly reconcile only alerts.
  - The S10 dispute wave: {SC['S10_dispute_wave']['vendor_calls']['disputes']:.1f} disputes per run at $15 each [G]. Profit {usd(SC['S10_dispute_wave']['profit'])} vs {usd(s1['profit'])}, with more Casey time ({SC['S10_dispute_wave']['casey_min_week']:.0f} min/week).
- **AI (Claude):**
  - About {SC['S01_normal']['vendor_calls']['claude']:.0f} calls per run, costing {usd(s1['total']['claude'])} per 60 days [M].
  - During the S06 outage, requests queue and keyword safety still fires. But the AI input contains homeowner details in every run (P3), which is the main AI finding.
  - Prompt-injection texts (S08) had no effect on money or details, because only fixed output fields are read.
- **License sites (ROC, Agriculture, city list):**
  - 'Unknown' results (CAPTCHA, down, or a layout change; S17g forces a layout change on day 20) send sign-ups to Casey, and his approval activates them with no identity code (B0019 example). That path, not the scraper, is the weakness.
  - The ROC-list variant (S17d) cut Casey time by about 7.5 min/week at equal profit.
- **Email and mail:** rarely used (identity fall-through only); letter costs are negligible.
""")
# ---------- open decisions
w("## 9. Open decisions (Part 13) [M, sim; same-seed paired differences ± standard error]\n")
w("| # | Decision | Result | Recommendation |\n|---|---|---|---|")
dec = [
("1", "24 h window: weekdays only vs every day", f"S17c − S01 profit {pd('S17c_weekday_window - S01_normal')} per 60 d; 365-day: {pd('L_weekdays - L_base')}/yr (not significant)", "Weekdays only: a slight edge, more quotes, and fairer to small firms. Make the 9 p.m. promise match the window."),
("2", "Free first win: all vs first 90 days", f"365-day: {pd('L_first90 - L_base')}/yr; businesses kept {pd('L_first90 - L_base','biz_kept')}", "First-90-days, if you accept a smaller business pool; otherwise keep it for all until the hand-run test measures joining."),
("3", "Recurring at 2 vs 4 months", f"365-day: {pd('L_rec4 - L_base')}/yr; businesses kept {pd('L_rec4 - L_base','biz_kept')}; confirm {pd('L_rec4 - L_base','picks_confirmed_share')}", "4 months: the largest profit lever found. Watch pool and landscape churn."),
("4", "$25/$75/$199 vs alternatives", f"alt5 − base {pd('S16_alt5_s15 - S16_base_s15')}; alt2 − base {pd('S16_alt2_s15 - S16_base_s15')}; alt1 − base {pd('S16_alt1_s15 - S16_base_s15')} per 60 d", "Keep $25/$75/$199 with recurring at 4 months (= alt5). $300 top tier: no clear gain. Avoid flat $25 and $19. Get a legal view on tiers first (see Licensing)."),
("5", "Partial refund if recurring stops within 30 days", f"365-day: {pd('L_rec_partial - L_base')}/yr", "Not favoured on money; adopt only if the hand-run test shows early stops hurt business trust."),
("6", "Unlicensed categories: insurance required vs 'on file yes/no'", f"S17e − S01 profit {pd('S17e_insurance_req - S01_normal')}; businesses kept {pd('S17e_insurance_req - S01_normal','biz_kept')}; active fakes {pd('S17e_insurance_req - S01_normal','fakes_active')}", "Show 'insurance on file: yes/no' and fix identity checks (problem 3). Requiring certificates costs profit and businesses; the fake reduction is better achieved by identity fixes."),
("7", "Live ROC lookups vs daily lists + live check at pick", f"S17d − S01 profit {pd('S17d_roc_lists - S01_normal')}; Casey {pd('S17d_roc_lists - S01_normal','casey_min_week')} min/wk; fakes {pd('S17d_roc_lists - S01_normal','fakes_active')}", "Daily lists + live check at the pick: same money, less Casey time, robust to layout changes (S17g)."),
]
for r in dec: w("| " + " | ".join(cell(c) for c in r) + " |")
w(f"\nAlso measured: all fixes on (S17a) − S01: profit {pd('S17a_all_fixes - S01_normal')}, Casey {pd('S17a_all_fixes - S01_normal','casey_min_week')} min/week, active fakes {pd('S17a_all_fixes - S01_normal','fakes_active')}. Approved hand-run scope (S17b) − S01: profit {pd('S17b_approved_scope - S01_normal')}, businesses kept {pd('S17b_approved_scope - S01_normal','biz_kept')}, confirm share {pd('S17b_approved_scope - S01_normal','picks_confirmed_share')}.\n")
# ---------- limits
w("""## 10. What the model can't tell us, and what the hand-run test must measure
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
""")
# ---------- code
w("## 11. Code\n")
w("**Where it is.** The full code is about 250 KB, which does not fit in a report under 90 KB. It is all in `sim_v93/` and inside `sim_v93_results.zip` (code, configs and every raw result file). The SHA-256 values below let you confirm the files are unchanged. Standard-library Python 3 only.\n")
w("**Re-run:**\n```\ncd sim_v93\npython3 run_all.py      # every scenario, seeds 1-50 (~5 min on 8 cores); writes results/raw_*.json\npython3 analyze.py      # results/summary.json\npython3 paired.py       # results/paired.json\npython3 churn.py        # results/churn.json\npython3 report.py       # this report\npython3 trace.py \"{}\" 1 1126   # outside-world timeline of one job (any reproduction above)\n```\n")
w("**Seeds:** 1–50 for every Part 10 scenario and S17a–h; 1–20 for the 365-day Part 13 runs; 1–8 for the demand sweep. Printed per run in `results/run_log.txt`.\n")
w("| File | Bytes | SHA-256 (first 16) | Role |\n|---|---|---|---|")
roles = {"config.py": "All Given/Assumed parameters, each labelled G or A", "engine.py": "Discrete-event clock, timers, business-hours maths", "vendors.py": "Fake Twilio, Stripe, Claude, ROC/Ag/city sites, email, mail (with failure modes)",
         "app.py": "App base: DB, texting layer, consent, quiet hours, status machine", "app_intake.py": "Inbound texts, opt-outs, safety, intake questions, cancel", "app_flow.py": "Cards, quotes, picks, confirm, charges, refunds, reconcile",
         "app_biz.py": "Business sign-up, identity chain, ROC checks, masked calls, Casey queue", "world.py": "World truth, homeowner generation, outside-world records", "world_people.py": "Homeowner personas and their reactions",
         "world_biz.py": "Businesses (join/leave/quote/confirm), fakes, gamers, Casey and backup", "sim.py": "Builds one run from overrides + seed", "metrics.py": "Money by month, cash curve, workload, Part 9 metrics",
         "checks.py": "Every Part 11 check, judged from outside-world records only", "scenarios.py": "Scenario definitions (Part 10 + S17a–h, S16 grid)", "run_all.py": "Runs everything on 8 processes", "rerun_one.py": "Re-runs one scenario",
         "analyze.py": "Aggregates raw results", "paired.py": "Same-seed differences", "churn.py": "Churn by category (growth)", "trace.py": "Reproduction timelines", "report.py": "Builds this report"}
for f in sorted(glob.glob(os.path.join(D, "*.py"))):
    b = open(f, "rb").read(); n = os.path.basename(f)
    w(f"| {n} | {len(b):,} | `{hashlib.sha256(b).hexdigest()[:16]}` | {roles.get(n, '')} |")
w("\n**Scenario definitions (`scenarios.py`, in full):**\n```python\n" + open(os.path.join(D, "scenarios.py")).read().strip() + "\n```\n")
w("*End of report. Every figure is simulated; nothing here was observed in the real world.*\n")
out = "\n".join(o)
open(os.path.join(D, "Havasu_Sim_v93_Review.md"), "w").write(out)
print("bytes", len(out.encode()), "checks pass/fail", npass, nfail)
