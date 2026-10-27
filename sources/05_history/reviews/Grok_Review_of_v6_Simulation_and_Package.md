# Havasu Home Quotes: everything for Claude's review (Oct 26, 2026)

**From:** Casey Solomon, with Grok Bot. **Contents, in priority order:**

1. Grok Bot's review of the Plan v6 simulation (ran the code, 50 seeds, reproduced all 27 scenarios, plus stress tests)
2. Stress-test results (simulated, 50 seeds): full summary CSV and the patched-code results
3. Code patch used for the leakage fix (diff against the package's sim.py)
4. The new stress scenarios added (JSON, only the ones not in the original scenarios.json)
5. Grok Bot's earlier review of the automated-plan package (Havasu Home Quotes Independent Review Package)

The original simulation package (config.json, scenarios.json, sim.py, run_all.py) is unchanged and not repeated here. Every simulation number is simulated, not measured.

**Casey's current standing decisions that both reviews check against:** Ask Hava is not part of the business and gets no traffic; the income goal is $20k–$50k a month; no homeowner name, phone or address goes into any AI tool; all legal and LLC work is deferred to one file, with a hard gate before any contractor signs, any live charge or any real homeowner request; plans use phases and relative days, not calendar dates.

---


---

# Part 1. Plan v6 simulation review

# Havasu Home Quotes, Plan v6: independent simulation review

**Reviewer:** Grok Bot. **For:** Casey Solomon (shareable with Claude).
**Package reviewed:** `upload_1405.md` (Part 6: four files, extracted byte-exact).
**Run on:** Monday, Oct 26, 2026, mid-afternoon PDT (America/Los_Angeles).

Every number in this report is **simulated**: invented homeowners, invented contractors, 50 seeds per scenario. Each item is tagged **[RUN]** (I executed it) or **[READ]** (I inferred it from reading code, config, or law). Nobody was contacted and no real service was used.

---

## The short answer

- **Reproduced: yes.** **[RUN]** All 27 scenarios match Part 4 to the dollar and to the rounded percentage. The seeds are deterministic, so 27 of 27 are within tolerance.
- **Verdict: NOT YET for v6 as written.** The Stage 0 shape itself (4 weeks, under $300, no live charges) is a sound, cheap test. But v6's Stage 0 contains four steps that run **before the legal/LLC hard gate** or reverse approved decisions:
  - filing carrier (10DLC) registration;
  - collecting Pro order forms;
  - an AI that hears homeowners' names, phones, and addresses;
  - opening four towns, plus handyman and pool.
  
  Re-scope Stage 0 to the approved hand-run test and it is ready. Detail is in section I.
- **The model cannot answer the questions that matter most.** **[READ + RUN]** These are the top 5 issues:
  1. **Your hours constrain nothing.** Recruiting happens automatically, so "Casey on overtime" *raises* profit (casey_unavailable: $3,083 vs $2,963, simulated). Realistic recruiting with drives to Bullhead and Kingman is about 12–20 h/week in months 3–6 (my estimate), against decision #15's cap of 40 h/month.
  2. **Leakage and collusion can't cost money in the code.** That is why leakage_high *beats* base. Patched so that picks can bypass the fee, 24% leakage cuts month-24 profit by about 20% (simulated, $2,353 vs $2,955).
  3. **There is no price response and no contractor capacity limit.** Fees +50% shows +39% profit with zero downside. 10x demand is served by the same 77 contractors at the same quote rate.
  4. **The $20k/month goal is 6–10x away.** Base month 24 is $2,963/mo (simulated), $17,037 short. Reaching $20k took about **6x demand plus 3x supply** (simulated $19,553, with 226 contractors and Casey at 22 h/week in month 6) or **10x demand** ($22,049). That means roughly 300–500 Lake Havasu City contacts a month, or about 17–29% of owner households every year. That is unrealistic for one local lead service.
  5. **Fixed costs are understated, and the cash buffer is thin.** $185/mo is about $174 of insurance plus $11, with no phone line, texting registration, hosting, or AI platform. At an estimated $450/mo, 80% of runs dip below $0 cash (simulated), because only about $600 remains after the $1,900 start-up.
- **The "Pro = a third of revenue" result rests on Pro perks.** **[READ]** These sit in tension with "no pay-to-win":
  - a +5-point pick bonus;
  - 1.25x pick weight;
  - an exemption from no-win churn;
  - overflow requests created from the Pro's own callers (about $2,066 of intro revenue over 2 years, simulated).
  
  The Pro product is an AI receptionist that hears homeowners' personal details.

---

## Conflicts with your current decisions

| Your standing decision | What v6 says or does | Finding |
|---|---|---|
| **Ask Hava gets no traffic and is not part of the business** | `demand._note`: city ramp is "v2.2 Base ramp x 0.625 for Ask Hava at zero". No config key routes demand from Ask Hava. | **Already zeroed.** **[RUN]** `stress_no_ask_hava` (explicit) is identical to v6_base: $2,963 month-24, 555 paid intros. Effect of zeroing: none beyond what v6 already did. The 25 partner referrals/month at month 24 are partners, not Ask Hava. |
| **Income goal $20k–$50k a MONTH** | Base month-24 profit is $2,963/mo before the 25% tax set-aside. | **Gap $17,037/mo** (simulated). Every scenario's gap is in the tables (column "Gap to $20k"). The best original scenario (`high`, $5,246) is still $14,754 short. **Demand needed:** about 6x the v6 ramp with 3x recruiting and cap 15 per trade-town ($19,553), or 10x with today's supply rules ($22,049, which is only possible because the code has no contractor capacity limit). $50k/mo is outside anything tested. |
| **No homeowner name/phone/address in any AI tool** | Part 2: "An AI assistant takes the details" by call or text. Pro: an "AI receptionist for the contractor's own missed calls", which hears homeowner details by design. Spanish intake and "the AI releases held requests" also run on raw homeowner messages. | **Conflict.** See the PII-safe design below. **[RUN]** `stress_pii_safe_intake` (web form + touch-tone, more person requests) costs about $286/mo at month 24 (simulated $2,677 vs $2,963), 10% of the profit. |
| **Legal/LLC deferred; hard gate before any contractor signs, any live charge, or any real homeowner request** | Part 3: "Carrier registration filed in Stage 0". `gates_v6.new_towns`: "the Stage 0 Pro order-form gate (20% of pitched, at least 4)". | **Conflict, two items.** 10DLC registration needs the business entity and the "new legal review before any automation, 10DLC texting, AI voice" (`deferred-legal-and-llc.md`). A Pro **order form** is a contractor signing, which belongs after the gate (and after day 45 per decision #12). The practice cards and "no live charges" parts respect the gate. |
| **No hard calendar dates; phases and relative days only** | Plan text uses months 1–24, "Stage 0", "day 45". The only date is the package's own document date. `config.start_month_of_year = 3` is a modeling input for seasonality, not a plan date. | **OK.** Keep it that way in any rewrite. Prefer "phase / day N" over "month N" in operating plans too. |
| **Approved hand-run test** (no automated phone line; Lake Havasu City only; licensed trades only; general repair on hold) | v6 adds an AI phone/text line, handyman, pool, three more towns from month 4, a Pro plan, opening a trade at 2 contractors, and a 4-week Stage 0. | Each is listed below as your choice, with its risk. |

### v6 changes vs the approved hand-run test (your choices, risk judged)

| Change (Casey's choice) | Approved test | Risk judgment |
|---|---|---|
| AI phone/text intake line | #6: no automated phone line or texting; Google Voice by hand | **High.** It needs 10DLC, an AI-voice legal review, and recording/AI disclosure, and it puts homeowner PII into AI (above). Texting refusal alone costs about 13% (simulated `texting_refused`). |
| Handyman trade | #3: licensed trades only; general repair on hold | **High (legal/trust).** Handyman work is usually done under the Arizona ROC "handyman" exemption, A.R.S. §32-1121(A)(14): **aggregate contract price under $1,000 including labor and materials**, casual or minor work, **no local building permit**, not part of a larger job, no splitting contracts to stay under the limit. The worker's ads must include **"not a licensed contractor"**. Subsection D removes the exemption for gas-connected work and fire-safety installs. So handyman pros are **not licensed** and conflict with the licensed-only rule. The platform would need a job-size screen (under $1,000, no permit, no gas) and the disclosure on every handyman quote and comparison. **[READ]** |
| Pool trade | not in the approved trades | **Medium.** Routine cleaning and chemical service is generally not ROC work. Pool equipment repair (pumps, heaters, pool plumbing and electrical) generally needs an ROC license unless it fits the under-$1,000, no-permit exemption, and gas heaters never fit it (§32-1121(D)(2)). Confirm the license class with the ROC before listing. Wave 2 adds pest control, which is licensed by the AZ Dept. of Agriculture Office of Pest Management (§32-1121(A)(8)), not the ROC, so the license check differs. Appliance work touching gas is never exempt. **[READ]** |
| Bullhead City, Fort Mohave, Kingman from month 4 | #17: no expansion unless the test passes | **Medium-high.** Expansion drives most of the recruiting hours (60-mile drives) and churn exposure. `v6_pro_gate_new_towns` shows delaying towns costs only about $26/mo at month 24 (simulated). Keep #17. |
| Pro plan ($199 AI receptionist) | #12: survey only; no deposits, payments, or sales until after day 45 | **High.** It is the product with the most PII exposure, it needs its own legal review, and it carries a third of simulated revenue on unproven conversion (60%). |
| Open a trade at 2 contractors | Go-live when each open trade has 5+ signed | **Medium.** 2 contractors means a comparison of at most 2 and fragile coverage. `v6_open_at_3`: −$223/mo (simulated); the test standard is stricter still. |
| Stage 0 = 4 weeks | Test = 45 days (+15-day extension) | **Low.** 4 weeks is too short for the planned sample size; the checklist already expects fewer than about 10 pay decisions by day 45. |

### PII-safe design (keeps AI to request IDs only)

1. **Intake without AI:** a web form (with photos) plus a touch-tone phone menu ("press 1 for plumbing…"), with no speech-to-text of names or addresses. Voicemails go to Casey by hand.
2. **Contact vault in plain code:** a small database holds name, phone, address, and ZIP. Everything else sees only `REQ-1234`, the trade, the ZIP, and a job description scrubbed by deterministic regex (phones, emails, street numbers, names list) **before** any AI sees it.
3. **Template texts, not AI-written texts,** to homeowners: fixed wording with `{request_id}`, `{quote_count}`, `{business_name}` filled in by code.
4. **AI only on the contractor side and on scrubbed text:** ranking or formatting quotes, bouncing quotes with phone numbers. Spanish works through a Spanish web form and Spanish templates.
5. **Pro receptionist:** no AI on homeowner calls. Offer missed-call **text-back from templates**, or drop it from the business until a legal review approves a design.
6. **Relay calls:** a number-masking proxy keyed by request ID; no recording or transcription.

---
## A. Could you run the code?

**Yes.** **[RUN]**
- **Setup:** Python 3.13.5, standard library only, 8 CPU cores.
- **Original run:** `python3 run_all.py 50`, 27 scenarios x seeds 1–50, 15.8 s wall clock.
- **Stress run:** 16 scenarios x 50 seeds, 36 s.
- **Patched bug-check run:** 4 scenarios x 50 seeds, about 2 s.
- **Extraction:** the four Part 6 files were extracted byte-exact. Fences were cut by line span and none were nested.
- **Integrity:** the original `scenarios.json` is unchanged; its SHA-256 `bb27b7ca…` matches a pre-run backup. My scenarios live in copies: `scenarios_stress.json` and `stress/scenarios.json`.

## B. Reproduction

- **Within tolerance: all 27.** **[RUN]** Every Part 4 row matches my `results/scenario_summary.csv` exactly at the printed rounding. Examples:
  - v6_base: 19.4% / 51.9% / 9.2% / 554.8 paid / $2,962.9 / $8,872.7 / $26,374.2 / 5.6 / 7.2 / 3.6 / 0%.
  - low: $1,223.9, 34% of runs below $0.
- **Outside tolerance: none.**
- **Rounding-only nits.** A few rows sit exactly at x.5%, so they print one point differently depending on rounding. Examples: `v6_precommit_unpopular` "2+ q in 4 h" is 42.5% (Part 4 shows 43%), and "No quote" for texting_refused and casey_unavailable is about 9.5%. These are not real differences.
- **Why it matches exactly:** each run uses `random.Random(seed)` with seeds 1..50 and no other randomness. Reproduction proves only that the code runs as shipped, not that the assumptions are right.

Month-24 detail behind v6_base **[RUN, simulated means over 50 seeds]**:

| Item | Value |
|---|---|
| Contacts | 164 |
| Requests | 115 |
| Paid intros | 46.5 |
| Intro fees | $2,423 (average $52) |
| Pro subscriptions | $1,118 (5.6 paying) |
| Refunds | −$51 |
| Stripe | −$119 |
| AI usage | −$107 |
| Pro usage | −$106 |
| Fixed costs | −$185 |
| Disputes | −$16 |
| **Profit** | **$2,963** |

Over two years:
- 1,463 requests, 45 of them duplicates;
- 82 overflow requests created from Pro contractors' own callers;
- 473 Casey hours in total.

---

## C. Code bugs found (ranked by how much they change conclusions)

All line numbers refer to `sim.py` as shipped (SHA-256 `3c3d5621…`).

### C1. Owner time is logged but never limits anything (severity: conclusion-changing)
- **Where:** `recruit()` L176–185; `add_contractor()` L227; budget checks L858–865.
- **What is wrong:**
  - The number of recruits each month comes straight from the `recruits_per_month` schedule.
  - Each signed contractor books 150 minutes at day 10 of the month whether or not you are on shift or over budget (L227 calls `casey()` without `casey_next_free`).
  - The budget checks only write a "problem" line.
  - Shift days are fixed (L109 `d % 7 < 4`), with no rotation, even though the config note says "rotating".
- **Effect [RUN]:**
  - `casey_unavailable` (6 x 12 h shifts) shows **higher** profit than base: $3,083 vs $2,963, simulated.
  - The only way the model can feel owner time is if recruiting stops: `stress_stop_recruiting_m7` gives $1,657 (−44%).
- **Real-world estimate [READ]:**
  - Months 3–6 sign about 15 contractors a month (LHC 5, Bullhead 4, Fort Mohave 2, Kingman 4).
  - At 150 minutes each that is 37.5 h/month, about 8.7 h/week.
  - About 10 of those 15 are about 60 miles away. At 2.5 visits each and about 2 h round trip, unbatched travel adds up to about 50 h/month.
  - So realistic recruiting-season load is **about 12–20 h/week**, not 7.2. That is above decision #15's 40 h/month.
- **Fix:**
```python
# in recruit(): cap signings by owner minutes available this month
TRAVEL = {"Lake Havasu City": 20, "Bullhead City": 300, "Fort Mohave": 300, "Kingman": 300}  # min per signed (2.5 visits)
budget = self.c["casey"]["weekly_budget_hours_recruiting_season"] * 60 * 4.3 - self.casey_month_minutes(m)
for town, sched in ...:
    for _ in range(n):
        cost = self.c["casey"]["minutes"]["per_contractor_signed"] + TRAVEL[town]
        if budget < cost: self.prob("recruit_blocked_by_owner_time", town); break
        budget -= cost; self.add_contractor(town, m)
# and in add_contractor(): book at casey_next_free(), on off-shift weekday business hours (8-17)
```

### C2. Leakage and collusion cannot reduce revenue (severity: conclusion-changing)
- **Where:** `on_day14()` L782–799.
- **What is wrong:** leakage is rolled only for requests that were **not paid and had quotes**. A paid pick can never bypass the fee, and leakage can only *add* money through invoices. The real failure, a picked contractor saying "no longer available" and then doing the job directly, isn't modeled.
- **Effect [RUN]:**
  - `leakage_high` (3x leakage) **beats** base: $2,982 vs $2,963 at month 24; year 2 $26,855 vs $26,374.
  - In a patched copy (`stress/patched/`), a share of pre-committed picks equal to the leakage rate goes direct:
    - base: month-24 $2,833 (−4%), year 2 $24,134 (−8%);
    - leakage 24%: month-24 **$2,353 (−20%)**, year 2 $20,422 (−22%), simulated.
- **Fix (as patched):**
```python
# on_pick(), before the pre-commit charge
if pre and self.rng.random() < self.c["homeowner"].get("pick_bypass_prob", 0.08):
    self.stat(self.t, "leakage_on_pick"); req["closed"] = True; return
```

### C3. No price response anywhere (severity: high for any fee or Pro-price decision)
- **Where:** L654–657.
- **What is wrong:** with `quote_precommits_fee` on, `yes_rate_by_fee` is replaced by a flat 0.93. `p_quote` does not depend on the fee, and homeowners never see the fee.
- **Effect [RUN]:**
  - `stress_fee_up50_naive`: +50% fees gives month-24 **$4,124 (+39%)** with identical quote and pick behavior. Any fee change "wins" in this model.
  - `stress_fee_up50_reaction`, with a modest contractor reaction (quote 15% less, 20% back out, 5% churn), lands at $2,935, roughly break-even with base, and service falls to 42% 2+ quotes in 4 h.
- **Fix:** make `p_quote` and pre-commit availability functions of fee, e.g. `pq *= (fee/45) ** -elasticity` with elasticity of about 0.5–1.0. Calibrate elasticity from the Stage 0 practice-card survey.

### C4. No contractor capacity (severity: high for growth and the $20k question)
- **Where:** `eligible()` L230–231, invites, `p_quote`.
- **What is wrong:** a contractor's quote probability and speed are the same at 1 invite a week or 50.
- **Effect [RUN]:** `goal_demand_x10` serves 13,944 requests with 77 contractors and still gets 52% 2+ quotes in 4 h. Heat-wave and growth results are optimistic.
- **Fix:** keep a 7-day invite count per contractor and use `pq *= max(0.2, 1 - invites_7d / capacity)`, with capacity of about 10–15 job cards a week for a 1–3 truck shop.

### C5. "Promise kept" is 100% by construction (severity: medium; it hides homeowner experience)
- **Where:** `on_promise_check()` L610–619; summary L877–888.
- **What is wrong:** the check fires before the deadline and sets `answered_t` with a "none yet" message, so `promise_kept` is always 1.0. "Never silence" is guaranteed by definition, not by performance.
- **Effect:** 9% of served homeowners get **no quote at all** (simulated base), and 19% are turned away. Those are the real experience numbers.
- **Fix:** report `share_with_comparison_by_deadline` and `share_homeowner_got_a_contractor_call`:
```python
got_comp = frac(lambda r: r["comparison_t"] is not None and r["comparison_t"] < self.promise_deadline(r["t"]), served)
```

### C6. Duplicates are "caught" but still served and charged (severity: medium-low)
- **Where:** L442–443. The stat is recorded, but the request continues.
- **Effect [RUN, patched]:** closing duplicates moves paid intros from 553 to 534 (−3.4%) and month-24 profit from $2,955 to $2,882 (−2.5%). In real life a duplicate would charge a contractor twice for one job.
- **Fix:** `req["closed"] = True; req["answered_t"] = t; return` after the stat, and exclude these from `turned_away`.

### C7. Pro advantages and overflow (severity: medium; inflates Pro's share)
- **Where:**
  - +0.05 homeowner selection bonus when a Pro quote is present (L600–601);
  - x1.25 pick weight for Pro quotes (L631);
  - Pro exempt from the no-win churn multiplier (L269);
  - overflow requests: 1 per Pro per month injected as new marketplace demand (L327–330).
- **Effect [RUN, base totals]:**
  - 82 overflow requests, about $2,066 of intro revenue over 2 years (simulated).
  - These are the Pro's **own** missed callers being sent to competitors. It is unclear contractors would opt in, and it routes homeowner PII through AI.
- **Fix:** set the bonuses to 0 unless the slot is real and shown to all, and make overflow opt-in with a share well below 1.

### C8. Pre-commit charges 1 minute after the pick, not after the 60-minute check (severity: low)
- **Where:** L664 `self.push(self.t + 1, "yes", ...)`.
- **What is wrong:** the rule says the card is charged unless the contractor says "no longer available" within 60 minutes. The code charges at once, so connection timing is up to 60 business minutes early.
- **Fix:** `self.push(window_end, "yes", rid, ch["k"])`.

### C9. Possible double loss on refunded-then-disputed intros (severity: low, about 1%)
- **Where:** L763–767.
- **What is wrong:** the dispute roll happens in `contact()` regardless of refund. A lost dispute deducts the fee again even if the fee was already refunded.
- **Fix:** skip the dispute roll when `refund` is True.

### C10. Smaller items (no material effect)
- **L455:** `held_strong_model_release_prob * 0 + held_uncertain` is dead arithmetic. Write `c["homeowner"]["held_uncertain"]`.
- **Nudge does nothing:** `on_nudge()` L542–546 records a stat only.
- **24-hour reminder missing:** the "24-hour reminder" (config `supply._note_fixes`) is not implemented. Its effect is folded into `contact_attempts_dist` by hand.
- **paid_intros counting:** `paid_intros` (L889) counts requests. A re-offer after a no-call (L752–757) can charge a second contractor on the same request, so revenue counts it but the paid-intro count doesn't.
- **Unused config keys:**
  - `business_clock.days`
  - `supply.responsiveness.p_question_instead`
  - `homeowner.selection_bonus_pro_slot` (the code uses `pro.slot_booking_selection_bonus`)
  - `assistant.promise_deadline`
  - `assistant.connected_min_seconds`
  - `refunds.rule`
  - `refunds.ineligible_request_credit`
  - `pro.spam_share`
  - `gates_v6.*`
- **Calendar drift:** `MONTH = 30 days` (L15–16), so 24 months is 720 days and calendar-month seasonality drifts about 5 days a year.
- **Turned-away definition (L870–871):** closed duplicates would be counted as turned away (seen in the patched run: 19% → 22%).

---

## D. Rule mismatches (code vs Part 2 rules)

| Part 2 rule | Code | Lines |
|---|---|---|
| Charge unless "no longer available" **within 60 minutes** | Charged 1 minute after pick | L657–664 |
| Connected = human answers **and 45 s or more** | Any answer counts; `connected_min_seconds` unused | L714–716 |
| Nudge at 60 minutes | No-op | L542–546 |
| Trade opens with 2 contractors **who returned a practice card** | Counts any active contractor; no practice-card step | L236–245 |
| **No pay-to-win** (no more invitations) | Invitations are fair, but Pro gets a pick bonus, pick weight, and churn exemption | L600–601, L631, L269 |
| 4 x 10-hour shifts, **rotating** (config note) | Fixed days 0–3 of every week, 6:00–16:00 | L108–111 |
| Budget about 7 h/week, then 3.5 | Logged only; never limits work | L858–865 |
| 24-hour reminder when a paid contractor hasn't called | Not implemented | — |
| Duplicates caught | Counted, then served | L442–443 |
| Leakage invoices on $149 jobs only | Implemented. The **test kit's** provider terms charge the tier fee on any tier within 90 days, so v6 and the approved terms disagree. | L793 |
| Refund: 2 attempts on 2 different days within 72 h, decided at 96 h | Implemented correctly | L734–736 |
| Stripe fee kept on refunds | Correct (Stripe doesn't return it) | L152–157 (refund debits fee only) |

---
## E. Unrealistic assumptions [READ, with sources or reasoning]

| Config key | v6 value | Suggested | Reason / source |
|---|---|---|---|
| `money.fixed_monthly` | $185 (about $174 insurance + $11) | **$400–650** | Missing: a business phone number and texting registration, an AI voice/SMS platform, hosting, email/domain, bookkeeping. The earlier automated-plan review priced fixed tools at about $620/mo. The insurance figure is itself an estimate. `stress_real_fixed_costs` ($450 + $1.50/request): month-24 $2,604, **80% of runs go below $0 cash** (simulated). |
| `money.startup_cash` / `owner_starting_cash` | $1,900 of $2,500 | Keep 3 months of fixed costs in reserve | Leaves about $600 (plus the $500 Stripe reserve). The minimum operating cash in base is $398 (simulated). |
| Marketing / acquisition cost | **$0** (no key) | $150–500/mo, or an explicit hours cost | 164 contacts a month (base month 24) won't arrive free. Google Business Profile is likely unavailable to a lead-generation business (prior review). Rack cards, posts, and partner upkeep cost time and money. |
| `casey.minutes.per_contractor_signed` | 150 min | 150 LHC; **about 450** Bullhead, Fort Mohave, Kingman | Each is about 60 miles from Lake Havasu City. 2.5 visits at about 2 h round trip, unless batched. |
| `supply.responsiveness.p_quote_mean` / `median_response_min` | 0.55 / 75 min | 0.35–0.45 / 180–240 min (test it) | ZIP-only cards with no photos or address can't be priced. Most trades reply "trip fee + diagnose". Small 1–3 truck shops answer texts between jobs or in the evening. `slow_contractors` already drops 2+ quotes in 4 h to 18%. The approved test uses photos, which helps. |
| `supply.precommit_still_available_prob` | 0.93 | 0.75–0.85 until measured | Unproven, and it is the single largest gain (+$3,400 in year 2 per Part 4). "I accept the fee if chosen" on every quote is a new ask for contractors used to free bidding. |
| `supply.free_monthly_churn` | 3%/mo | 5–8%/mo | Contractors on lead services churn when lead quality disappoints. `contractor_churn_wave` (10%) halves month-24 profit (simulated). |
| `pro.trial_conversion` / `price` | 60% / $199 | 15–30% | Off-the-shelf missed-call text-back and AI receptionist tools are sold at roughly $29–349/mo (per the test kit's vendor list), with free trials. 60% trial conversion is high for small-business SaaS. |
| `pro.overflow_requests_per_pro_month` | 1.0 | 0–0.3 | Contractors rarely hand their own callers to competitors. It also needs PII handling. |
| `trades.hvac.summer_mult` + promise | 1.8x; quote by 9 p.m. next day | Same-day path for "AC out" June–September | Over 110°F in Havasu, "no AC" is urgent for older residents. A next-day promise loses those jobs, and an "emergency" label skips the card entirely (`emergency_share` 0.02). |
| `demand.lhc_ramp` month 24 | 50 contacts/mo | Treat as unknown until Stage 0 data | Base already implies about 3% of LHC owner households using the service each year. Every multiple of that is an untested claim. |
| Contractor capacity | unlimited | 10–15 cards/week per shop | See C4. |
| Fee elasticity | none | elasticity 0.5–1.0 | See C3. |

---

## F. Your new scenarios [RUN, 50 seeds each, all values simulated]

The copy is `scenarios_stress.json`. It is also `stress/scenarios.json`, which holds the original 27 plus 15 new scenarios. The original file is untouched. Same columns as Part 4, plus the gap to $20k a month:

| Scenario (simulated) | Turned away | 2+ q in 4 h | No quote | Paid intros 2 yrs | M24 profit | Year 1 | Year 2 | Pro m24 | Casey h/wk m6 | m18 | Runs cash<$0 | Gap to $20k/mo |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| v6_base | 19% | 52% | 9% | 555 | $2,963 | $8,873 | $26,374 | 5.6 | 7.2 | 3.6 | 0% | $17,037 |
| stress_no_ask_hava | 19% | 52% | 9% | 555 | $2,963 | $8,873 | $26,374 | 5.6 | 7.2 | 3.6 | 0% | $17,037 |
| stress_combined_bad_year | 26% | 16% | 18% | 216 | $391 | $-2,125 | $2,391 | 0.5 | 6.7 | 2.7 | 100% | $19,609 |
| stress_competitor_m9 | 23% | 42% | 14% | 323 | $1,427 | $7,024 | $14,781 | 4.2 | 7.3 | 3.4 | 0% | $18,573 |
| stress_fee_up50_naive | 19% | 52% | 9% | 555 | $4,124 | $12,740 | $36,512 | 5.6 | 7.2 | 3.6 | 0% | $15,876 |
| stress_fee_up50_reaction | 23% | 42% | 15% | 429 | $2,935 | $9,684 | $27,391 | 4.3 | 7.1 | 3.3 | 0% | $17,065 |
| stress_collusion_leakage | 20% | 52% | 9% | 464 | $2,747 | $7,718 | $23,787 | 6.4 | 7.0 | 3.5 | 2% | $17,253 |
| stress_stop_recruiting_m7 | 32% | 44% | 14% | 428 | $1,657 | $8,384 | $19,045 | 3.6 | 7.2 | 1.3 | 0% | $18,343 |
| stress_stripe_4pct | 19% | 52% | 9% | 555 | $2,924 | $8,733 | $26,018 | 5.6 | 7.2 | 3.6 | 0% | $17,076 |
| stress_real_fixed_costs | 19% | 52% | 9% | 555 | $2,604 | $5,346 | $22,344 | 5.6 | 7.2 | 3.6 | 80% | $17,396 |
| stress_pii_safe_intake | 20% | 52% | 9% | 469 | $2,677 | $7,720 | $23,254 | 5.4 | 7.1 | 3.6 | 0% | $17,323 |
| goal_demand_x3 | 19% | 52% | 10% | 1590 | $7,305 | $21,984 | $62,853 | 6.3 | 7.0 | 3.8 | 0% | $12,695 |
| goal_demand_x6 | 20% | 52% | 9% | 3159 | $13,613 | $41,636 | $118,093 | 6.4 | 7.3 | 4.3 | 0% | $6,387 |
| goal_demand_x10 | 20% | 52% | 9% | 5255 | $22,049 | $68,367 | $189,670 | 6.7 | 7.8 | 5.0 | 0% | $-2,049 |
| goal_demand_x6_supply | 9% | 59% | 4% | 3884 | $19,553 | $50,409 | $170,272 | 21.2 | 22.0 | 11.4 | 0% | $447 |
| goal_demand_x10_supply | 9% | 59% | 4% | 6406 | $30,133 | $78,856 | $261,378 | 21.0 | 22.3 | 12.3 | 0% | $-10,133 |

**Overrides JSON (exact, as run):**

- `stress_no_ask_hava`: Ask Hava gives zero traffic (explicit). config already has Ask Hava at zero, so this should match v6_base
  ```json
  {"demand":{"lhc_ramp":{"1":6.25,"3":12.5,"6":21.9,"9":28.1,"12":34.4,"24":50.0}}}
  ```
- `stress_combined_bad_year`: Everything goes a bit wrong at once: Low demand, slow contractors, 6% churn, Pro flops, 2x leakage, homeowners screen calls, real fixed costs $450, precommit 0.85
  ```json
  {"demand":{"lhc_ramp":{"1":4,"3":8,"6":14,"9":18,"12":22,"24":32},"partner_referrals_month24":15},"supply":{"responsiveness":{"p_quote_mean":0.45,"median_response_min":240},"free_monthly_churn":0.06,"precommit_still_available_prob":0.85},"pro":{"test_signups_month1":1,"monthly_trial_rate":0.01,"trial_conversion":0.3,"monthly_churn":0.12},"homeowner":{"leakage_prob":0.16,"answers_relay_call_prob":0.5,"returns_call_prob":0.3},"money":{"fixed_monthly":450}}
  ```
- `stress_competitor_m9`: A competitor (national lead site push or local copycat) arrives around model month 9: demand growth stops and falls back, partners split, contractors poached (5% churn), quote rate drops
  ```json
  {"demand":{"lhc_ramp":{"1":6.25,"3":12.5,"6":21.9,"9":28.1,"12":22,"24":25},"partner_referrals_month24":12},"supply":{"free_monthly_churn":0.05,"responsiveness":{"p_quote_mean":0.48}}}
  ```
- `stress_fee_up50_naive`: All intro fees +50%, nothing else changes (shows the model has no price response)
  ```json
  {"trades":{"plumbing":{"fee":68,"major_fee":224},"hvac":{"fee":88,"major_fee":224},"electrical":{"fee":68,"major_fee":224},"handyman":{"fee":44,"major_fee":44},"pool":{"fee":68,"major_fee":68},"roofing":{"fee":68,"major_fee":224},"pest":{"fee":68,"major_fee":68},"garage_door":{"fee":68,"major_fee":68},"irrigation":{"fee":68,"major_fee":68},"appliance":{"fee":44,"major_fee":44},"water_treatment":{"fee":68,"major_fee":224}}}
  ```
- `stress_fee_up50_reaction`: All intro fees +50% with a contractor reaction: quote 15% less, 15% of picked back out, 5% churn
  ```json
  {"trades":{"plumbing":{"fee":68,"major_fee":224},"hvac":{"fee":88,"major_fee":224},"electrical":{"fee":68,"major_fee":224},"handyman":{"fee":44,"major_fee":44},"pool":{"fee":68,"major_fee":68},"roofing":{"fee":68,"major_fee":224},"pest":{"fee":68,"major_fee":68},"garage_door":{"fee":68,"major_fee":68},"irrigation":{"fee":68,"major_fee":68},"appliance":{"fee":44,"major_fee":44},"water_treatment":{"fee":68,"major_fee":224}},"supply":{"responsiveness":{"p_quote_mean":0.47},"precommit_still_available_prob":0.8,"free_monthly_churn":0.05}}
  ```
- `stress_collusion_leakage`: Contractors and homeowners go around the fee: leakage 30%, picked contractors claim 'no longer available' then deal direct (precommit 0.70), invoices rarely paid
  ```json
  {"homeowner":{"leakage_prob":0.3},"supply":{"precommit_still_available_prob":0.7},"refunds":{"leakage_invoice_paid_prob":0.2}}
  ```
- `stress_stop_recruiting_m7`: Casey stops recruiting after month 6 (zero new contractors from month 7)
  ```json
  {"supply":{"recruits_per_month":{"Lake Havasu City":{"1":6,"2":5,"3":5,"4":2,"12":0,"24":0,"6":1.75,"7":0},"Bullhead City":{"4":4,"5":4,"6":3.0,"9":0,"24":0,"3":4,"7":0},"Fort Mohave":{"4":2,"5":2,"6":2.0,"9":0,"24":0,"3":2,"7":0},"Kingman":{"4":4,"5":3,"6":3.0,"9":0,"24":0,"3":4,"7":0}}}}
  ```
- `stress_stripe_4pct`: Card processing 4% + $0.30 instead of 2.9% + $0.30
  ```json
  {"money":{"stripe_pct":0.04}}
  ```
- `stress_real_fixed_costs`: Fixed costs that v6 leaves out: phone/number, texting registration, hosting, AI voice platform, email, bookkeeping (~$450/mo) and usage $1.50/request
  ```json
  {"money":{"fixed_monthly":450,"usage_per_request":1.5}}
  ```
- `stress_pii_safe_intake`: PII-safe design: web form + touch-tone menu instead of an AI talking to homeowners; lower voice completion, more person requests, cheaper usage
  ```json
  {"homeowner":{"intake_completion_voice":0.7},"demand":{"person_request_share":0.15},"money":{"usage_per_request":0.3}}
  ```
- `goal_demand_x3`: Demand (city ramp and partners) x3, supply rules unchanged
  ```json
  {"demand":{"lhc_ramp":{"1":18.75,"3":37.5,"6":65.7,"9":84.3,"12":103.2,"24":150.0},"partner_referrals_month24":75}}
  ```
- `goal_demand_x6`: Demand x6, supply rules unchanged
  ```json
  {"demand":{"lhc_ramp":{"1":37.5,"3":75.0,"6":131.4,"9":168.6,"12":206.4,"24":300.0},"partner_referrals_month24":150}}
  ```
- `goal_demand_x10`: Demand x10, supply rules unchanged
  ```json
  {"demand":{"lhc_ramp":{"1":62.5,"3":125.0,"6":219.0,"9":281.0,"12":344.0,"24":500.0},"partner_referrals_month24":250}}
  ```
- `goal_demand_x6_supply`: Demand x6 with cap 15 per trade-town and recruiting x3
  ```json
  {"demand":{"lhc_ramp":{"1":37.5,"3":75.0,"6":131.4,"9":168.6,"12":206.4,"24":300.0},"partner_referrals_month24":150},"supply":{"max_per_trade_town":15,"recruits_per_month":{"Lake Havasu City":{"1":18,"2":15,"3":15,"4":6,"12":3,"24":1.5},"Bullhead City":{"4":12,"5":12,"6":9,"9":6,"24":1.5,"3":12},"Fort Mohave":{"4":6,"5":6,"6":6,"9":3,"24":0.9,"3":6},"Kingman":{"4":12,"5":9,"6":9,"9":6,"24":1.5,"3":12}}}}
  ```
- `goal_demand_x10_supply`: Demand x10 with cap 15 per trade-town and recruiting x3
  ```json
  {"demand":{"lhc_ramp":{"1":62.5,"3":125.0,"6":219.0,"9":281.0,"12":344.0,"24":500.0},"partner_referrals_month24":250},"supply":{"max_per_trade_town":15,"recruits_per_month":{"Lake Havasu City":{"1":18,"2":15,"3":15,"4":6,"12":3,"24":1.5},"Bullhead City":{"4":12,"5":12,"6":9,"9":6,"24":1.5,"3":12},"Fort Mohave":{"4":6,"5":6,"6":6,"9":3,"24":0.9,"3":6},"Kingman":{"4":12,"5":9,"6":9,"9":6,"24":1.5,"3":12}}}}
  ```

**What each shows:**
- **stress_no_ask_hava.** Identical to base, so Ask Hava is already at zero in v6.
- **stress_combined_bad_year (the business breaks).**
  - Month-24 profit is $391/mo, year 1 is −$2,125, and **100% of runs go below $0 cash**.
  - Only 16% of homeowners get 2+ quotes in 4 h and 18% get no quote.
  - None of the individual risk tests in Part 4 shows this. The risks compound.
- **stress_competitor_m9.** Month-24 profit falls 52% to $1,427 and paid intros fall 42%. Any credible entrant (national lead sites already present, or a local copycat) roughly halves the business.
- **stress_fee_up50_naive vs stress_fee_up50_reaction.**
  - The naive run shows +39%, but only because the model has no price response (C3).
  - With a modest contractor reaction the gain disappears ($2,935) and service quality drops: 42% 2+ quotes, 15% no quote.
- **stress_collusion_leakage.**
  - In the shipped code this is −7% ($2,747). Most of the damage comes from the lower pre-commit availability, because code leakage is free (C2).
  - In the patched code, 24% leakage alone is −20%.
- **stress_stop_recruiting_m7.**
  - Month-24 profit falls 44% to $1,657. Active contractors at month 24 drop from 72 to 28 and 32% of homeowners are turned away.
  - Recruiting never "finishes", because churn of 3% plus no-win quits keeps draining supply.
- **stress_stripe_4pct.** −$39/mo (−1.3%). Card fees are not a threat at these ticket sizes.
- **stress_real_fixed_costs.** −$359/mo at month 24 and **−$3,527 in year 1**, and 80% of runs dip below $0 cash. The early months are the danger.
- **stress_pii_safe_intake.** −$286/mo (−10%) for the PII-safe design. That is the price of honoring the no-PII-in-AI rule, and it is affordable.
- **goal_demand_x3 / x6 / x10 and the _supply variants.**
  - $20k/month needs about 6x demand with 3x recruiting ($19,553), or 10x demand with impossible capacity ($22,049).
  - The x6_supply case needs **226 active contractors and Casey at 22 h/week in month 6, 11 h/week in month 18** (simulated, and before C1's travel time). That is far beyond decision #15.
  - Profit is close to linear in demand: about $2,100/mo of month-24 profit per +1x of the v6 ramp (x3 → x6 → x10), with Pro roughly fixed.

---

## G. Problems the simulation doesn't capture [READ]

**Business model**
- **Who owns the homeowner relationship?** After one introduction, the contractor has the homeowner's number (the relay connects them). Every repeat job is off-platform for free. Lifetime value per homeowner is one fee.
- **The quote is not a price.** Most home repair can't be priced from a ZIP and a sentence. Comparisons will be mostly "trip fee $89 + diagnosis". Homeowners may see little value over calling one plumber, and the "up to 3 quotes" promise feels hollow.
- **Competition already exists and is free to homeowners:** Angi/HomeAdvisor, Thumbtack, Google Local Services Ads, Nextdoor, Facebook groups, Yelp. Contractors already pay those for leads, and some refuse per-lead fees after bad experiences.
- **Two-sided cold start.** The model assumes demand arrives from month 1 regardless of contractor count, and vice versa. In reality each side waits for the other.
- **Seasonal residents.** Winter residents often use a home-watch company or property manager, who are the real buyers. There is a partner channel, but no partner economics.

**Homeowner and contractor experience**
- **Homeowners:**
  - Being told "no one quoted" (9%) or "not served" (19%) after giving details is a trust hit that spreads by word of mouth in a small town. Neither has a cost in the model.
  - A relay number calling from an unknown 928 line gets screened. `contact_failures` (40% answer) is the only proxy.
- **Contractors:**
  - The pre-commit "accept the fee if chosen" moves risk to the contractor before they've spoken to the customer. Expect disputes over "the homeowner just wanted a price".
  - Charging cards automatically after 60 minutes will generate chargebacks. `dispute_prob` 0.01 is low for disputed lead charges.

**Legal and trust**
- **AI handling homeowner PII** (intake, Spanish, held-request release, the Pro receptionist) conflicts with your rule. It also triggers recording and AI-disclosure duties. Arizona is a one-party-consent state for recording, but callers from all-party states may need disclosure.
- **Texting (TCPA/10DLC):** consent capture, STOP handling, and quiet hours are modeled, but registration timing is before the gate (see Conflicts).
- **Licensing exposure.** Referring homeowners to an unlicensed handyman for work over $1,000 or needing a permit (A.R.S. §32-1121(A)(14)) creates trust and possible liability exposure. The platform needs a screen and the "not a licensed contractor" disclosure. ROC license verification for licensed trades needs a refresh cadence (licenses lapse or are suspended).
- **Claims language.** "Never silence" and "licensed" must be literally true. The current metric (C5) can't tell you whether they are.
- **Non-circumvention.** The approved provider terms (90-day tier fee) and v6's "$149 jobs only" invoicing disagree. Pick one.

**Operations**
- **On-shift unavailability.** Your shift hours (6:00–16:00 in the model) overlap contractor and partner business hours, which is when recruiting, disputes, and outages happen. Outages during shifts already log 7.4 a run (simulated).
- **Single point of failure.** If you are unavailable for two weeks (illness, court, training), nothing in the model or plan covers it.
- **Data upkeep.** Keeping the contractor roster, licenses, insurance certificates, and coverage maps current is recurring work that isn't modeled.

**Money**
- **No marketing spend** (E).
- **Thin cash** (E).
- **Taxes.** The 25% set-aside is on deposits, not profit. That is conservative, but sales tax (TPT) treatment of Pro **subscriptions** (software/receptionist service) differs from intro fees and isn't considered.
- **Chargeback fees.** Real chargeback fees and Stripe's risk review for a new account charging saved cards without the cardholder present are not modeled. A Stripe hold would stop revenue.
- **Pro fixed costs per Pro** ($7) look low for an AI voice receptionist with a dedicated number and 300 minutes.

---

## H. Top 5 recommendations (ranked)

1. **Run Stage 0 as the approved hand-run test, not v6's Stage 0.**
   - Lake Havasu City only, licensed plumbing/electrical/HVAC only, Google Voice by hand, web form with photos, practice cards, no live charges, no order forms, no 10DLC filing.
   - Measure the four numbers the model can't know: contacts per week, quote rate and speed on real cards, pre-commit acceptance (`precommit_still_available_prob`), and a fee-elasticity survey.
   - *Effect:* it replaces the 3–4 assumptions that swing month-24 profit by ±50% (simulated) with real data, for under $300, and respects the legal gate.
2. **Adopt the PII-safe design (no AI on homeowner details).**
   - *Effect:* about −$286/mo at month 24 (simulated, `stress_pii_safe_intake`). It removes the largest legal and trust risk and the AI-voice legal review from the critical path.
3. **Fix C1–C5 in the sim before trusting any v7 numbers:** owner-time cap with travel, leakage on picks, fee elasticity, contractor capacity, and an honest promise metric.
   - *Effect:* the expected corrected base is lower. My partial patch alone gives −4% month 24 and −8% year 2 (simulated). Owner-time and capacity caps will cut further and will move the "best options" ranking.
4. **Budget real fixed costs and keep a cash reserve.**
   - Set `fixed_monthly` to about $450 and hold about 3 months of costs ($1,350+) before any live charge.
   - *Effect:* it avoids the 80% chance of hitting $0 cash in year 1 (simulated `stress_real_fixed_costs`).
5. **Rethink the path to $20k/month, separately from this test.**
   - At $52 average fee and about $47 net, $20k needs about 425 paid intros a month, 9x base.
   - Single-town intro fees can't reach the goal on any tested demand. The only scenarios near $20k need 6–10x demand and 226 contractors (simulated).
   - If Stage 0 passes, judge the business on contribution per hour of your time and on adding higher-value lines (for example, property-manager and home-watch partner accounts), not on more towns.
   - *Effect on homeowner experience:* fewer turned away (19% → about 9% with the denser supply in `goal_demand_x6_supply`, simulated).

---

## I. Verdict

**Not yet, for Plan v6 as written. Yes, for a re-scoped Stage 0 that matches the approved hand-run test.**

Why not yet:
- v6's Stage 0 files carrier (10DLC) registration and collects Pro order forms. Both are commitments that belong after the legal/LLC hard gate, and Pro sales come after day 45 (#12).
- Its design has an AI taking homeowners' names, phones, and addresses, which conflicts with your no-PII-in-AI rule.
- It widens scope beyond the approved test: handyman (unlicensed under §32-1121(A)(14), under $1,000, no permit), pool, three more towns (#17), and opening a trade at 2 contractors (go-live needs 5+). These are your choices, but each adds risk to a test whose job is to measure demand cheaply.
- The simulation's headline results (pre-commit gain, Pro third of revenue, 7 h/week, leakage harmless, fee increases free) rest on code gaps C1–C4 and unproven inputs, so they shouldn't set Stage 0 targets.

The Stage 0 *shape* (4 weeks, under $300, no live charges) is right. Run it, but extend toward the approved 45 days for sample size, and keep it in the approved scope.

---

## Appendix 1. Original 27 scenarios as rerun, with gap to $20k/mo [RUN, simulated]

| Scenario (simulated) | Turned away | 2+ q in 4 h | No quote | Paid intros 2 yrs | M24 profit | Year 1 | Year 2 | Pro m24 | Casey h/wk m6 | m18 | Runs cash<$0 | Gap to $20k/mo |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| v6_base | 19% | 52% | 9% | 555 | $2,963 | $8,873 | $26,374 | 5.6 | 7.2 | 3.6 | 0% | $17,037 |
| v5_as_written | 27% | 37% | 11% | 349 | $1,738 | $5,412 | $14,895 | 5.5 | 7.2 | 3.0 | 6% | $18,262 |
| low | 20% | 40% | 15% | 273 | $1,224 | $2,622 | $10,445 | 2.0 | 6.7 | 2.8 | 34% | $18,776 |
| high | 19% | 62% | 6% | 890 | $5,246 | $16,831 | $48,180 | 11.2 | 7.3 | 4.4 | 0% | $14,754 |
| heat_wave | 18% | 47% | 14% | 527 | $2,943 | $8,678 | $25,600 | 5.8 | 7.1 | 3.5 | 0% | $17,057 |
| voice_outages | 19% | 51% | 10% | 540 | $2,753 | $8,068 | $25,310 | 5.7 | 7.3 | 3.9 | 2% | $17,247 |
| texting_refused | 20% | 44% | 10% | 438 | $2,586 | $6,928 | $22,730 | 6.1 | 7.1 | 3.5 | 2% | $17,414 |
| contractor_churn_wave | 32% | 43% | 14% | 416 | $1,484 | $6,598 | $15,030 | 2.4 | 7.1 | 2.8 | 0% | $18,516 |
| spam_flood | 20% | 52% | 10% | 525 | $2,902 | $8,066 | $25,469 | 6.4 | 7.0 | 3.6 | 0% | $17,098 |
| pro_flop | 20% | 51% | 10% | 517 | $2,042 | $5,079 | $17,084 | 1.0 | 6.7 | 2.9 | 4% | $17,958 |
| demand_spike | 19% | 53% | 9% | 1057 | $5,148 | $14,426 | $45,300 | 6.7 | 7.1 | 4.0 | 0% | $14,852 |
| thin_supply | 30% | 46% | 13% | 443 | $2,152 | $6,321 | $18,856 | 4.0 | 4.0 | 2.2 | 2% | $17,848 |
| casey_unavailable | 19% | 52% | 10% | 551 | $3,083 | $8,861 | $26,523 | 6.1 | 7.1 | 3.5 | 0% | $16,917 |
| slow_contractors | 20% | 18% | 15% | 431 | $2,329 | $6,849 | $21,497 | 5.5 | 7.1 | 3.5 | 4% | $17,671 |
| pro_heavy_users | 19% | 53% | 9% | 551 | $2,939 | $8,079 | $26,365 | 5.9 | 7.2 | 3.6 | 6% | $17,061 |
| leakage_high | 20% | 52% | 9% | 549 | $2,982 | $8,431 | $26,855 | 5.7 | 7.2 | 3.7 | 0% | $17,018 |
| contact_failures | 20% | 51% | 9% | 550 | $2,912 | $7,856 | $25,220 | 5.6 | 7.1 | 3.6 | 0% | $17,088 |
| v6_assistant_back | 19% | 52% | 9% | 555 | $2,663 | $8,183 | $22,780 | 5.6 | 7.2 | 3.0 | 0% | $17,337 |
| v6_no_precommit | 19% | 51% | 10% | 441 | $2,569 | $7,151 | $23,003 | 5.7 | 7.1 | 3.6 | 6% | $17,431 |
| v6_precommit_cuts_quotes | 19% | 43% | 13% | 505 | $2,793 | $7,692 | $24,712 | 5.9 | 6.9 | 3.4 | 2% | $17,207 |
| v6_neighbors_as_v5 | 22% | 52% | 10% | 533 | $2,934 | $8,602 | $27,102 | 6.5 | 7.3 | 3.7 | 0% | $17,066 |
| v6_pro_gate_new_towns | 16% | 51% | 10% | 471 | $2,937 | $5,523 | $23,275 | 5.6 | 2.0 | 4.3 | 0% | $17,063 |
| v6_cap_4 | 21% | 51% | 10% | 528 | $2,785 | $8,423 | $24,500 | 5.2 | 4.7 | 3.4 | 0% | $17,215 |
| v6_wave2_never | 26% | 56% | 7% | 524 | $2,804 | $8,873 | $25,406 | 5.8 | 7.2 | 2.9 | 0% | $17,196 |
| v6_serve_with_1_or_2 | 11% | 47% | 13% | 583 | $3,081 | $9,260 | $28,184 | 5.9 | 7.4 | 3.6 | 0% | $16,919 |
| v6_open_at_3 | 26% | 54% | 8% | 512 | $2,740 | $7,959 | $24,629 | 5.0 | 7.0 | 3.5 | 2% | $17,260 |
| v6_precommit_unpopular | 19% | 42% | 13% | 463 | $2,546 | $7,065 | $22,994 | 5.7 | 7.0 | 3.5 | 0% | $17,454 |

## Appendix 2. Patched-code check (bugs C2 and C6) [RUN, simulated]

`stress/patched/sim.py` adds two opt-in patches: close duplicates, and a share of pre-committed picks bypassing the fee. The control below differs slightly from the shipped v6_base because the patch consumes one extra random draw per pick.

| Scenario | Paid intros | M24 profit | Year 2 |
|---|---|---|---|
| v6_base (patched code, patches off) | 553 | $2,955 | $26,315 |
| patched_dups_only | 534 | $2,882 | $26,159 |
| patched_base (dups closed + 8% of picks bypass) | 484 | $2,833 | $24,134 |
| patched_leakage_high (24% leakage, picks included) | 398 | $2,353 | $20,422 |

## Appendix 3. Files

All files are in `/workspace/homerepair/sim_v6/`:
- the four original files, untouched;
- `results/` from the original run;
- `scenarios_stress.json` (27 + 15 scenarios);
- `stress/`: code copies, scenarios, and `results/`;
- `stress/patched/`: the patched sim and its results;
- this report;
- `sim_v6_results.zip`.

To rerun the stress set:
```bash
cd stress && python3 run_all.py 50 $(cat names.txt)
```

---

# Part 2. Stress-test results (simulated, 50 seeds each)

## 2a. All 42 scenarios, original code (`stress/results/scenario_summary.csv`)

```csv
scenario,about,requests,unserved_share,two_quotes_within_4h,zero_quotes,promise_kept,paid_intros,paid_per_request,month24_profit,year1_profit,year2_profit,min_operating_cash,pro_paying_m24,active_contractors_m24,casey_hours_week_m6,casey_hours_week_m18,pro_pick_share,pro_quote_share,month24_profit_p10,month24_profit_p90,share_runs_cash_below_zero
v6_base,Plan v6: Plan v5 plus all simulation fixes (the settings in config.json),1463.04,0.194,0.519,0.092,1.0,554.82,0.379,2962.897,8872.713,26374.247,397.623,5.62,72.44,7.176,3.593,0.124,0.115,2242,3839,0.0
stress_no_ask_hava,"Ask Hava gives zero traffic (explicit). config already has Ask Hava at zero, so this should match v6_base",1463.04,0.194,0.519,0.092,1.0,554.82,0.379,2962.897,8872.713,26374.247,397.623,5.62,72.44,7.176,3.593,0.124,0.115,2242,3839,0.0
stress_combined_bad_year,"Everything goes a bit wrong at once: Low demand, slow contractors, 6% churn, Pro flops, 2x leakage, homeowners screen calls, real fixed costs $450, precommit 0.85",893.34,0.263,0.161,0.175,1.0,216.4,0.242,390.844,-2125.069,2390.992,-3160.495,0.54,47.56,6.695,2.667,0.032,0.03,74,763,1.0
stress_competitor_m9,"A competitor (national lead site push or local copycat) arrives around model month 9: demand growth stops and falls back, partners split, contractors poached (5% churn), quote rate drops",974.02,0.23,0.422,0.141,1.0,322.56,0.331,1426.675,7024.312,14781.044,367.853,4.18,56.12,7.26,3.358,0.128,0.118,797,2135,0.0
stress_fee_up50_naive,"All intro fees +50%, nothing else changes (shows the model has no price response)",1463.04,0.194,0.519,0.092,1.0,554.82,0.379,4124.252,12739.677,36512.208,441.967,5.62,72.44,7.176,3.593,0.124,0.115,3170,5269,0.0
stress_fee_up50_reaction,"All intro fees +50% with a contractor reaction: quote 15% less, 15% of picked back out, 5% churn",1451.02,0.232,0.419,0.148,1.0,429.08,0.296,2935.002,9683.507,27391.305,386.388,4.3,57.74,7.099,3.339,0.116,0.109,2125,3801,0.0
stress_collusion_leakage,"Contractors and homeowners go around the fee: leakage 30%, picked contractors claim 'no longer available' then deal direct (precommit 0.70), invoices rarely paid",1456.38,0.195,0.518,0.088,1.0,463.94,0.318,2746.694,7718.076,23787.222,366.564,6.4,72.58,7.041,3.5,0.133,0.126,2071,3551,0.02
stress_stop_recruiting_m7,Casey stops recruiting after month 6 (zero new contractors from month 7),1444.72,0.319,0.438,0.138,1.0,427.5,0.296,1656.84,8383.501,19044.978,397.623,3.6,28.32,7.176,1.275,0.161,0.15,1065,2344,0.0
stress_stripe_4pct,Card processing 4% + $0.30 instead of 2.9% + $0.30,1463.04,0.194,0.519,0.092,1.0,554.82,0.379,2923.881,8733.224,26017.837,395.387,5.62,72.44,7.176,3.593,0.124,0.115,2212,3789,0.0
stress_real_fixed_costs,"Fixed costs that v6 leaves out: phone/number, texting registration, hosting, AI voice platform, email, bookkeeping (~$450/mo) and usage $1.50/request",1463.04,0.194,0.519,0.092,1.0,554.82,0.379,2604.002,5346.009,22343.744,-330.125,5.62,72.44,7.176,3.593,0.124,0.115,1890,3480,0.8
stress_pii_safe_intake,"PII-safe design: web form + touch-tone menu instead of an AI talking to homeowners; lower voice completion, more person requests, cheaper usage",1265.78,0.2,0.525,0.092,1.0,469.12,0.371,2676.784,7720.067,23253.873,400.606,5.4,72.06,7.126,3.589,0.122,0.112,1998,3510,0.0
goal_demand_x3,"Demand (city ramp and partners) x3, supply rules unchanged",4241.28,0.194,0.516,0.095,1.0,1590.28,0.375,7305.356,21984.45,62853.018,460.323,6.32,76.36,7.001,3.838,0.117,0.108,6240,8333,0.0
goal_demand_x6,"Demand x6, supply rules unchanged",8363.6,0.195,0.517,0.091,1.0,3158.64,0.378,13612.662,41635.617,118092.704,555.126,6.44,76.92,7.317,4.258,0.121,0.112,11978,15365,0.0
goal_demand_x10,"Demand x10, supply rules unchanged",13943.66,0.195,0.52,0.093,1.0,5254.58,0.377,22048.734,68366.776,189669.741,678.401,6.68,77.12,7.805,4.996,0.123,0.115,19608,24429,0.0
goal_demand_x6_supply,Demand x6 with cap 15 per trade-town and recruiting x3,8543.82,0.091,0.593,0.045,1.0,3884.16,0.455,19552.936,50409.09,170272.459,791.313,21.16,226.12,22.011,11.39,0.093,0.085,17610,21430,0.0
goal_demand_x10_supply,Demand x10 with cap 15 per trade-town and recruiting x3,14090.64,0.094,0.592,0.044,1.0,6405.84,0.455,30133.151,78855.748,261377.589,1072.189,21.0,228.46,22.295,12.262,0.094,0.087,27891,32314,0.0
```

## 2b. Patched code, leakage and duplicate fixes (`stress/patched/results/scenario_summary.csv`)

```csv
scenario,about,requests,unserved_share,two_quotes_within_4h,zero_quotes,promise_kept,paid_intros,paid_per_request,month24_profit,year1_profit,year2_profit,min_operating_cash,pro_paying_m24,active_contractors_m24,casey_hours_week_m6,casey_hours_week_m18,pro_pick_share,pro_quote_share,month24_profit_p10,month24_profit_p90,share_runs_cash_below_zero
v6_base,original logic (control),1469.78,0.193,0.516,0.094,1.0,553.28,0.376,2955.035,8653.283,26315.301,381.866,5.66,72.84,7.013,3.647,0.126,0.118,2129,3842,0.0
patched_dups_only,duplicates closed instead of served,1455.96,0.219,0.519,0.092,1.0,533.64,0.366,2882.042,8224.513,26158.919,367.984,6.1,72.4,7.006,3.631,0.13,0.118,2179,3832,0.0
patched_base,duplicates closed + 8% of paid picks bypass the fee,1454.96,0.219,0.512,0.095,1.0,484.5,0.333,2833.087,7320.811,24133.582,356.618,5.96,73.62,6.937,3.578,0.122,0.113,1798,4191,0.0
patched_leakage_high,duplicates closed + leakage 24% on both unpaid and picked,1462.38,0.226,0.526,0.093,1.0,397.58,0.272,2353.246,6451.859,20421.767,358.528,5.72,73.06,7.016,3.516,0.123,0.114,1736,2959,0.02
```

(Month-by-month profit CSVs are in sim_v6_results.zip.)

---

# Part 3. Code patch (sim.py, original vs patched)

```diff
--- stress/sim.py	2026-10-26 14:10:29.042537751 -0700
+++ stress/patched/sim.py	2026-10-26 14:11:43.292968637 -0700
@@ -441,6 +441,9 @@
         self.money_out(t, c["money"]["usage_per_request"] * 0.6, "usage")
         if self.rng.random() < d["duplicate_share"]:
             self.stat(t, "duplicates_caught")
+            if c.get("patch", {}).get("close_duplicates"):
+                req["closed"] = True; req["answered_t"] = t; req["duplicate"] = True
+                return
         if not self.trade_open(town, trade, m):
             req["closed"] = True
             req["answered_t"] = t
@@ -658,6 +661,11 @@
             window_end = self.add_business(self.t, self.c["assistant"].get("precommit_check_business_min", 60))
         else:
             window_end = self.add_business(self.t, self.c["assistant"]["yes_window_business_min"])
+        if pre and self.rng.random() < self.c.get("patch", {}).get("pick_bypass_prob", 0):
+            # PATCH: picked contractor says 'no longer available' and does the job directly with the homeowner (fee lost)
+            self.stat(self.t, "leakage_on_pick")
+            req["closed"] = True
+            return
         if pre:
             # the quote already accepted the fee: the card is charged unless the contractor replies 'no longer available' within the check window
             if self.rng.random() < yes_p:
```

Patched scenarios file:

```json
{
 "v6_base": {
  "about": "original logic (control)",
  "overrides": {}
 },
 "patched_dups_only": {
  "about": "duplicates closed instead of served",
  "overrides": {
   "patch": {
    "close_duplicates": true
   }
  }
 },
 "patched_base": {
  "about": "duplicates closed + 8% of paid picks bypass the fee",
  "overrides": {
   "patch": {
    "close_duplicates": true,
    "pick_bypass_prob": 0.08
   }
  }
 },
 "patched_leakage_high": {
  "about": "duplicates closed + leakage 24% on both unpaid and picked",
  "overrides": {
   "patch": {
    "close_duplicates": true,
    "pick_bypass_prob": 0.24
   },
   "homeowner": {
    "leakage_prob": 0.24
   }
  }
 }
}
```

---

# Part 4. New stress scenarios added to scenarios.json (one per line)

```json
{
"stress_no_ask_hava":{"about":"Ask Hava gives zero traffic (explicit). config already has Ask Hava at zero, so this should match v6_base","overrides":{"demand":{"lhc_ramp":{"1":6.25,"3":12.5,"6":21.9,"9":28.1,"12":34.4,"24":50.0}}}},
"stress_combined_bad_year":{"about":"Everything goes a bit wrong at once: Low demand, slow contractors, 6% churn, Pro flops, 2x leakage, homeowners screen calls, real fixed costs $450, precommit 0.85","overrides":{"demand":{"lhc_ramp":{"1":4,"3":8,"6":14,"9":18,"12":22,"24":32},"partner_referrals_month24":15},"supply":{"responsiveness":{"p_quote_mean":0.45,"median_response_min":240},"free_monthly_churn":0.06,"precommit_still_available_prob":0.85},"pro":{"test_signups_month1":1,"monthly_trial_rate":0.01,"trial_conversion":0.3,"monthly_churn":0.12},"homeowner":{"leakage_prob":0.16,"answers_relay_call_prob":0.5,"returns_call_prob":0.3},"money":{"fixed_monthly":450}}},
"stress_competitor_m9":{"about":"A competitor (national lead site push or local copycat) arrives around model month 9: demand growth stops and falls back, partners split, contractors poached (5% churn), quote rate drops","overrides":{"demand":{"lhc_ramp":{"1":6.25,"3":12.5,"6":21.9,"9":28.1,"12":22,"24":25},"partner_referrals_month24":12},"supply":{"free_monthly_churn":0.05,"responsiveness":{"p_quote_mean":0.48}}}},
"stress_fee_up50_naive":{"about":"All intro fees +50%, nothing else changes (shows the model has no price response)","overrides":{"trades":{"plumbing":{"fee":68,"major_fee":224},"hvac":{"fee":88,"major_fee":224},"electrical":{"fee":68,"major_fee":224},"handyman":{"fee":44,"major_fee":44},"pool":{"fee":68,"major_fee":68},"roofing":{"fee":68,"major_fee":224},"pest":{"fee":68,"major_fee":68},"garage_door":{"fee":68,"major_fee":68},"irrigation":{"fee":68,"major_fee":68},"appliance":{"fee":44,"major_fee":44},"water_treatment":{"fee":68,"major_fee":224}}}},
"stress_fee_up50_reaction":{"about":"All intro fees +50% with a contractor reaction: quote 15% less, 15% of picked back out, 5% churn","overrides":{"trades":{"plumbing":{"fee":68,"major_fee":224},"hvac":{"fee":88,"major_fee":224},"electrical":{"fee":68,"major_fee":224},"handyman":{"fee":44,"major_fee":44},"pool":{"fee":68,"major_fee":68},"roofing":{"fee":68,"major_fee":224},"pest":{"fee":68,"major_fee":68},"garage_door":{"fee":68,"major_fee":68},"irrigation":{"fee":68,"major_fee":68},"appliance":{"fee":44,"major_fee":44},"water_treatment":{"fee":68,"major_fee":224}},"supply":{"responsiveness":{"p_quote_mean":0.47},"precommit_still_available_prob":0.8,"free_monthly_churn":0.05}}},
"stress_collusion_leakage":{"about":"Contractors and homeowners go around the fee: leakage 30%, picked contractors claim 'no longer available' then deal direct (precommit 0.70), invoices rarely paid","overrides":{"homeowner":{"leakage_prob":0.3},"supply":{"precommit_still_available_prob":0.7},"refunds":{"leakage_invoice_paid_prob":0.2}}},
"stress_stop_recruiting_m7":{"about":"Casey stops recruiting after month 6 (zero new contractors from month 7)","overrides":{"supply":{"recruits_per_month":{"Lake Havasu City":{"1":6,"2":5,"3":5,"4":2,"12":0,"24":0,"6":1.75,"7":0},"Bullhead City":{"4":4,"5":4,"6":3.0,"9":0,"24":0,"3":4,"7":0},"Fort Mohave":{"4":2,"5":2,"6":2.0,"9":0,"24":0,"3":2,"7":0},"Kingman":{"4":4,"5":3,"6":3.0,"9":0,"24":0,"3":4,"7":0}}}}},
"stress_stripe_4pct":{"about":"Card processing 4% + $0.30 instead of 2.9% + $0.30","overrides":{"money":{"stripe_pct":0.04}}},
"stress_real_fixed_costs":{"about":"Fixed costs that v6 leaves out: phone/number, texting registration, hosting, AI voice platform, email, bookkeeping (~$450/mo) and usage $1.50/request","overrides":{"money":{"fixed_monthly":450,"usage_per_request":1.5}}},
"stress_pii_safe_intake":{"about":"PII-safe design: web form + touch-tone menu instead of an AI talking to homeowners; lower voice completion, more person requests, cheaper usage","overrides":{"homeowner":{"intake_completion_voice":0.7},"demand":{"person_request_share":0.15},"money":{"usage_per_request":0.3}}},
"goal_demand_x3":{"about":"Demand (city ramp and partners) x3, supply rules unchanged","overrides":{"demand":{"lhc_ramp":{"1":18.75,"3":37.5,"6":65.7,"9":84.3,"12":103.2,"24":150.0},"partner_referrals_month24":75}}},
"goal_demand_x6":{"about":"Demand x6, supply rules unchanged","overrides":{"demand":{"lhc_ramp":{"1":37.5,"3":75.0,"6":131.4,"9":168.6,"12":206.4,"24":300.0},"partner_referrals_month24":150}}},
"goal_demand_x10":{"about":"Demand x10, supply rules unchanged","overrides":{"demand":{"lhc_ramp":{"1":62.5,"3":125.0,"6":219.0,"9":281.0,"12":344.0,"24":500.0},"partner_referrals_month24":250}}},
"goal_demand_x6_supply":{"about":"Demand x6 with cap 15 per trade-town and recruiting x3","overrides":{"demand":{"lhc_ramp":{"1":37.5,"3":75.0,"6":131.4,"9":168.6,"12":206.4,"24":300.0},"partner_referrals_month24":150},"supply":{"max_per_trade_town":15,"recruits_per_month":{"Lake Havasu City":{"1":18,"2":15,"3":15,"4":6,"12":3,"24":1.5},"Bullhead City":{"4":12,"5":12,"6":9,"9":6,"24":1.5,"3":12},"Fort Mohave":{"4":6,"5":6,"6":6,"9":3,"24":0.9,"3":6},"Kingman":{"4":12,"5":9,"6":9,"9":6,"24":1.5,"3":12}}}}},
"goal_demand_x10_supply":{"about":"Demand x10 with cap 15 per trade-town and recruiting x3","overrides":{"demand":{"lhc_ramp":{"1":62.5,"3":125.0,"6":219.0,"9":281.0,"12":344.0,"24":500.0},"partner_referrals_month24":250},"supply":{"max_per_trade_town":15,"recruits_per_month":{"Lake Havasu City":{"1":18,"2":15,"3":15,"4":6,"12":3,"24":1.5},"Bullhead City":{"4":12,"5":12,"6":9,"9":6,"24":1.5,"3":12},"Fort Mohave":{"4":6,"5":6,"6":6,"9":3,"24":0.9,"3":6},"Kingman":{"4":12,"5":9,"6":9,"9":6,"24":1.5,"3":12}}}}}
}
```

---

# Part 5. Earlier review: automated-plan package

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
