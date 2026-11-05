> Origin: Casey's handoff package "Ask_Hava_CLAUDE_MASTER_HANDOFF_2026-11-03" (dated November 3, 2026), copied unchanged on November 5, 2026 (D-1499). A proposal, not an executed test; any instruction inside is the package's own, not a task or rule for this repository; see `docs/reviews/2026-11-03-handoff-zip-review.md`.

> **NOV 3 FOUNDER OVERRIDE — EXECUTION RULE:** The sole current handoff entrypoint is `00_CONTROL/00_READ_FIRST.md`; see `00_CONTROL/01_SCOPE_AUTHORITY_AND_DEFERRED_WORK.md`. Live-person/real-customer pilots, recruitment experiments, live transaction trials and market-go/no-go criteria are **not authorized for Claude's present work**. Historical pilot proposals in this file are *archived research, not tasks, acceptance tests, prerequisites or executable directions*. Development, safety verification and synthetic War Games continue. Historical fee, legal, governance and security constraints remain binding as reconciled with the live repository.

---

# WG-06 — Early-quote introduction-fee discount: 30% within four hours (PROPOSAL FOR TESTING)

**Added:** 2026-11-03 at founder request. **Status:** `PROPOSED_FOR_WAR_GAMES_ONLY`; **not approved for implementation, customer-facing advertising, pricing changes, ledger changes, production rollout, or contractor charging.** Claude must return evidence and options for founder approval before changing FEE-003/FEE-008 or any signed fee policy. This experiment supplements WG-01–WG-05, not overrides them.

## Founder hypothesis and a precise interpretation

The hypothesis is that a **30% discount on Ask Hava's introduction fee** for an eligible contractor who submits a timely, genuine quote **within four hours of receiving a job invitation** will produce faster, usable homeowner quotes and improve contractor participation. This is **not** a 30% discount on the contractor's project price, and the contractor still pays **nothing unless selected and a payable introduction is confirmed** under the standing transaction process.

On a hypothetical *paid selected job* with no other waivers, the arithmetic would be:

| Existing tax-included intro fee | 30% reduction | Hypothetical resulting fee |
|---:|---:|---:|
| $25.00 | $7.50 | $17.50 |
| $75.00 | $22.50 | $52.50 |
| $199.00 | $59.70 | $139.30 |

Fee tier determination remains the current approved method: the job's eligible basis (including FEE-003 midpoint for a range, and the approved recurring-service four-month/cap rule) determines the **base fee first**; the experimental 30% adjustment is assessed afterward. FEE-008's published amounts are **tax-included**. Test tax and rounding in cents, without silently applying a surcharge or altering established tax rules. Zero-dollar first wins, repeat-customer exemptions and fee-free jobs remain **zero** and must not be converted into paid jobs or consume extra free-job allowances. Do not allow stacked promotions beyond a specifically tested and approved policy.

## Experimental branches (no winner presumed)

- **Control:** current $25/$75/$199 fees, no early-quote discount.
- **Candidate:** a 30% discount only if the contractor submitted a sufficiently complete, deliverable, non-placeholder quote within the qualifying four-hour window **and** later became the payable, selected contractor.
- **Optional comparator arms** if simulated evidence indicates that 30% may sacrifice too much margin: 10% and 20%, plus either a longer window or trade-specific window. Do not multiply costly simulations gratuitously; use existing WG-05 stop/cost guardrails and comparable paired seeds.
- **Timing policy remains undecided:** compare four consecutive wall-clock hours against four hours during published contractor business hours. The message's send event, verified provider delivery event, bounce, and unknown-delivery states are different; **never silently penalize a contractor for a message never delivered**. Record the chosen clock, event provenance, and timezone, including weekend/holiday and daylight-savings boundaries. Do not assign the discount using client device time or email-open pixels.

## What counts as a qualifying early quote (test policy alternatives before choosing)

A valid submission must contain the mandatory quote fields appropriate to the job and trade—real price or honest provisional range, scope/inclusions/exclusions, practical timing, and any required verification. A one-character placeholder, implausibly low bait quote, unsigned price, irrelevant category, empty estimate, or automated spam cannot reserve a discount. Larger jobs requiring a site visit should not be forced into made-up firm prices: compare a reasonable provisional/range-quote policy, category exemption, or longer window. Later minor edits and homeowner-requested clarifications should not automatically erase an earned benefit; test material changes, bait-and-switch revisions, cancellations, withdrawals, and job reclassification separately. Require transparent audit logs and a deterministic rule Claude proposes for the eventual founder decision.

## What to measure and how to judge it

Report, by trade, existing fee tier, market pocket (Phoenix/Scottsdale/Goodyear etc. per WG-03), and Lake Havasu comparison: median and p90 time to **first valid** quote; share receiving at least one and multiple valid quotes within 4, 24, and 48 business hours; homeowner picks/completed paid introductions; quote completeness, change/cancellation/dispute rates; contractor response and retention; and **actual Hava contribution margin** after discounts, provider fees, refunds, support, messaging, and outreach. Record homeowner satisfaction where simulated vs observed evidence is clearly labeled. Distinguish **submitted** from **received**, **valid**, **selected**, and **paid**. Measure whether the early responders merely cannibalize quotes that would have arrived anyway, whether slow but high-quality contractors are driven off, and whether the incentive unfairly favors firms able to quote instantly.

For the present authorized work, **simulation only**. No live pilot or real-person experiment is assigned. Synthetic effects are hypotheses, not causal proof; no permanent fee policy change without a separate explicit founder approval. Do not infer ROI from quote volume alone. A discount is valuable only if incremental valid quotes, selections, retention or lower operational cost justify the revenue concession.

## Proposed additional adversarial cases: TX-106–TX-115

The machine-readable companion is `57_EARLY_QUOTE_INCENTIVE_SCENARIOS.json`. These are **10 additional proposed cases**, bringing the full documented TX plan from **105 to 115**; none is executed. Deduplicate against current repository cases before coding. All cases inherit WG-03 market-segmentation, WG-05 early stop, and D-1154 $40 AI budget. No production emails, real-money charges, personal information or live feature rollout.

1. **TX-106:** compare control vs 30% arm in paired contractor/job cohorts; measure valid-quote latency, downstream picks, margin.
2. **TX-107:** test exact four-hour threshold, late/early event ordering, retries, duplicated and out-of-order delivery callbacks, atomic recording of eligibility.
3. **TX-108:** weekend, late-night, holiday, time-zone, and delayed/failed-delivery fairness; compare wall-clock and contractor-business-hour branches.
4. **TX-109:** rapid placeholder/bait bids, automated spam, lowball then scope inflation, intentionally non-comparable quotes to capture discount.
5. **TX-110:** revisions after early valid bid, homeowner changed scope, contractor withdrawal and replacement quote; distinguish harmless correction from material repricing.
6. **TX-111:** fee calculation and audit: $25/$75/$199, boundary/range/recurring basis, zero/free-job treatment, tax-included rounding, voids/refunds and idempotent Stripe-to-ledger reconciliation.
7. **TX-112:** scarce-trade and large-job site-visit cohorts; does the short deadline reduce quote quality or exclude responsible contractors?
8. **TX-113:** tier-boundary manipulation, job splitting or padded estimated prices to compensate for the discount; adverse-selection impact on honest contractors.
9. **TX-114:** contractor and homeowner collusion, repeat accounts, self-dealing, send-time manipulation and notification replay; no unearned promotions or personal-info release.
10. **TX-115:** end-to-end cross-market economics over multiple cohorts including conversion, retention, coverage, price transparency and founder/operator workload; detect simulated ROI hallucinations and stop duplicate known failures.

## Founder decision gate

After the war games, provide a decision memo with (a) **No discount**, (b) **30% / 4 eligible hours**, and (c) a lower-discount or different-window alternative when evidence supports it. Show uncertainty, proposed wording and anti-gaming terms, fee math and ledger impacts, and required live pilot design. **Do not silently implement the proposal or replace existing $25/$75/$199 standing policy**. Claude should request approval under the project's decision process if it recommends adopting a price promotion.
