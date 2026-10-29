# Ask Hava handoff: start here (October 28, 2026)

> **Note (D-482):** where this file says 7.6%, the sales-tax set-aside is 8.6% (FEE-046, D-444). The spec wins.


> **Status, October 28, 2026:** sections 5 and 6 and steps 2 and 3 of section 8 are done in fix round 14 (D-429 to D-449); see `docs/PREBUILD.md` section 2. The ChatGPT sales-tax answer is in `docs/for-casey-review.md`; the Grok knowledge base is in `docs/catalog/trade_kb/`.

This file is for a **new chat** picking up Ask Hava. Read all of it before doing anything. It tells you where the plan stands, the rules Casey has set, the product changes he just asked for, and what to do next.

---

## 1. What Ask Hava is

A quote-request service in Lake Havasu City, Arizona (askhava.com).

- A homeowner texts or web-chats a job.
- Every eligible opted-in local business gets an anonymous job card **by email**.
- Businesses quote. The homeowner sees every quote and picks one.
- The picked business confirms **and pays in one step**:
  - $25 under $500
  - $75 for $500–$1,500
  - $199 over $1,500
  - Ongoing services count as 4 months, capped at $75.
  - First win is free in the first 90 days. Repeat work is free.
- Only then does the business get the homeowner's first name and address, plus masked calls.
- Homeowners never pay.

**Ask Hava is not a contractor.** It's a middleman that never prices, schedules or supervises work.

Standard statement: "Ask Hava is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly."

## 2. Where things stand

- **The plan is in this repo:**
  - `docs/spec/*.md` holds ~610 requirements.
  - `docs/decisions.md` runs to D-428.
  - `kit/tasks.json` holds 161 build tasks, and `docs/legal/` holds 8 legal drafts.
  - `docs/catalog/` holds the license map, the question bank and the categories.
  - `docs/PREBUILD.md` is the single to-do list.
- **Nothing is built and nothing is live.**
  - The build is on hold at **Gate P**: Casey replies "approved to build" on GitHub issue #24.
  - Both scheduled build runners are **paused**. Leave them paused until Casey approves the build.
- **Repo checks must pass on every PR:**
  - Required CI checks: `Ci`, `Twin`, `Ally`.
  - Local checks: `python3 scripts/{check_catalog,check_legal_docs,trace_spec,validate_tasks,pii_scan}.py`, `python3 scripts/gen_prompts.py --check`, and `python3 -m pytest -q`.
  - GraphQL is blocked, so use REST: `gh api repos/casey451/askhava/pulls ...`.
  - Enable auto-merge with `gh api -X PUT repos/casey451/askhava/pulls/<n>/ccr/auto_merge -f merge_method=squash`.
  - **Never touch branch `claude/kit-state`** (the build runner's state).

## 3. Casey's standing rules (all still in force)

- **Nothing outward without Casey's explicit go:** no sign-ups, purchases, outreach, publishing, live messages, live charges or deployments. AI never creates accounts or accepts terms for him.
- **No secrets** (passwords, API keys) in chat or plain files.
- **No homeowner names, phones, addresses or photos** go into an AI tool that keeps data. Use request IDs.
- **Don't claim something was tested** unless it actually was.
- **No simulations or persona passes** without talking to Casey first.
- **Never raise outside-employment or employer-policy topics.**
- **Company setup comes later:** the LLC, insurance and trademark go on the "later" list (Casey's decisions, Oct 28).
- **Look for the ideal solution, not just hard rules.** Challenge a rule when a better way exists. Don't raise far-fetched risks.
- **One license rule for every trade:** research whether a license is needed; if so, licensed businesses only; otherwise any legitimate business. Ask Hava advertises no specific trades.
- **Texts use a direct, plain speaking style** (Hormozi/Martell-like style only). No impersonation, never named or quoted.
- **No personal name** in user-facing copy.
- **No calls to businesses, ever.** Businesses are email-first.
- **How to talk to Casey:** non-technical, step by step, one question at a time.

## 4. Decided today (Oct 28), already merged

- **D-417 Pest control:**
  - The business attests to its Department of Agriculture license at sign-up.
  - Cards say the license is "not independently verified".
  - No letter to the Department.
  - This isn't fully wired through yet; see gap G5.
- **D-418 Defaults confirmed:** B2, B4–B9, B12 and B14–B16 in `docs/PREBUILD.md` section 4.
- **D-419 to D-428 Texting and terms:**
  - Texting in is the consent; there's no "Reply OK".
  - Terms are accepted by tapping Pick, with only "By picking, you agree to the Terms" beside the button. The $100 cap stays inside the Terms.
  - After a request goes out, ask once: updates by text, email or both. (Replaced in fix round 15: updates go by text, and the quote page offers email, D-460.)
- **Trademark:** skipped for now.
- **Insurance:** later, after the business is working.
- **Spanish:**
  - English-only conversations at launch.
  - Spanish safety, STOP and HELP texts stay on, checked by back-translation in two AIs.
  - Full Spanish comes after a paid review.
- **Sales tax: open.** ChatGPT is researching it; the prompt is in `docs/handoff/ChatGPT_prompt_sales_tax.md`. No ADOR letter is planned. The 7.6% set-aside stays meanwhile.

## 5. NEW: Casey's product direction from the walkthrough review (main work for the next round)

Casey walked through `docs/handoff/walkthrough-storyboard.md` (the 42-slide "how it works" deck) and gave this direction. **Re-review the whole plan through this lens.** The goal, in his words:

> The information about the job going from homeowner to business, and the quotes going back to the homeowner, should be "as well packaged as possible", so homeowners feel the service knows exactly what they need, and businesses get a pre-packaged job they'd otherwise pay a well-versed receptionist to gather.

1. **Uniform quotes.**
   - Every quote for a job type sits on the same playing field, so quotes compare directly: a range for a remodel, a monthly price for pool cleaning, tiers for something like detailing.
   - Businesses can add notes, but notes are screened so they stay about the job and can't be used to pull the homeowner around Ask Hava.
2. **Expert intake that thinks like a licensed pro in every trade.**
   - It asks the sizing questions a pro would (for example, the bathroom size).
   - It asks for photos, judges whether they're good enough and asks for more if needed. If the homeowner can't send photos, the request still goes through.
   - It reads inspiration images (like Pinterest screenshots) and builds a short project file, for example "standard 5×8 master bath, this layout, minimalist modern, these textures".
   - For remodels, it can generate a concept image labeled "AI-generated, not exact, may not be feasible" and confirm with the homeowner ("Is this the kind of thing you want?"), with back-and-forth.
   - Grok is researching the trade knowledge this runs on; the prompt is in `docs/handoff/Grok_prompt_trade_expert_research.md`.
3. **Emergency handling only for fire-department situations.**
   - Flag only things like a gas smell, where you'd call a fire truck.
   - A flood goes straight to emergency plumbers.
   - An AC outage in the heat goes straight to HVAC businesses that take urgent jobs, **with no heat-safety or health advice**. Casey doesn't want liability for health or safety advice: "they are well aware the AC went out and it's hot."
   - The site states plainly that Ask Hava is not an emergency service and doesn't judge what needs an emergency response.
4. **Human-sounding copy everywhere**, like a sharp, experienced receptionist. Rewrite the templates. The not-a-contractor message still has to come across.
5. **Drop the own-or-rent question and the renter/landlord feature entirely**, unless research finds a real legal reason to keep it. "If you call a plumber, they don't ask you if you own the home."
6. **Short texts plus a personal portal.**
   - Texts carry only a brief synopsis plus a link to a personalized homeowner portal.
   - The portal holds all quotes, business info, license numbers and **review links (Google, Yelp)**.
   - No long texts.
7. **Business sign-up collects** the Google Business URL, Yelp URL and license numbers, and these flow onto every quote.
8. **No scheduling or arrival-time orchestration.**
   - Once the homeowner picks and the business pays, the business gets the contact details and sets the time itself.
   - Simplify the pick and confirm texts (for example, drop "has until 7:30 p.m. Arizona time to confirm" style detail from homeowner texts).
9. **Coverage messages.**
   - Tell the homeowner right away how many businesses the request went to ("we're sending this to the 5 window washers signed up with us").
   - Or say right away that we don't cover that trade yet.
   - When a trade has no businesses, email Casey's AI assistant so he can recruit. Recruiting stays email or in person, never texts or calls. (Updated in fix round 15: mostly self-serve from disclosed Facebook group posts plus a few visits, D-456.)
10. **Re-review everything** in the plan through this lens.

**These reverse earlier rules on purpose. Record each reversal as a decision with Casey's words:**

- AI-006 / TXT-004 / CARD-007: today AI writes nothing anyone reads, and cards are fixed fields only. The new direction is AI-written intake questions, AI-written job cards approved by the homeowner, and AI photo reading.
- PICK and WORD rules that show **no reviews**: the new direction adds review links.
- The heat-safety text (INT-031) and the 211 line: removed.
- The own-or-rent question (INT-028) and the renter text (TXT-028): removed, pending the legal check.
- Photos: there's no flow today (gap G2). The new direction makes photos a core part of intake.
- Quote types (PICK-002): replace free-form "in their own words" with a uniform format per job type, plus screened notes.

**What comes with these changes** (handle it in the same round, briefly, without blocking):

- Privacy policy and AI wording (AI-018): "No AI reads your messages today" must change.
- Zero-retention or provider terms for photos.
- Image-generation provider choice.
- Note screening rules.
- Section 230 and deception points from legal Memo 4: keep AI from diagnosing or pricing, and have the homeowner approve the card.
- The Twilio question may need the AI wording updated.

## 6. The 23 gaps found in the walkthrough

They're listed in full, with files and fixes, at the end of `docs/handoff/walkthrough-storyboard.md` (G1–G23). The top six:

- **G1:** Homeowners are never told it's free or that the business pays a fee.
- **G2:** Photos have no flow.
- **G3:** The summary isn't the card, and the business job page is undefined.
- **G4:** $25 site-visit quotes on big jobs.
- **G5:** Pest self-attestation isn't wired through.
- **G6:** There's no single ordered intake script.

Several gaps change or disappear under section 5. Resolve all 23 in the same round.

## 7. Waiting on outside answers

- **ChatGPT:** sales tax (TPT) research.
- **Grok:** trade-expert knowledge base (YAML per trade, photo playbook, test messages). Use it to build the question bank and intake.
- **Twilio:** short eligibility question, drafted in `docs/handoff/Twilio_question.md`. Not sent yet. Revisit it after the AI changes.
- **Stripe:** the eligibility and auto-confirm question is in `docs/for-casey-review.md`. Not sent yet.

## 8. What the next chat should do

1. Read this file, `docs/PREBUILD.md`, the storyboard, and the Grok and ChatGPT results Casey pastes in.
2. Write a short **rework plan** for section 5 plus the 23 gaps. Group it by homeowner, business, owner and AI, and list the decisions Casey must make, **one question at a time**. Before asking anything, look things up (like whether own-or-rent has any legal basis).
3. Apply it as one fix round (a PR), keeping all repo checks green. Update `docs/PREBUILD.md`.
4. Rebuild the walkthrough deck from the new plan so Casey can confirm it matches what he pictures.
5. **No simulations** without Casey's OK. The build stays on hold.

## 9. Where else things live

These are from the previous session's workspace and may not exist in a new one. The repo copies above are the source of truth.

- **The walkthrough deck (artifact):** "Ask Hava: How It Works" in Casey's artifacts.
- **Master plan:** `docs/PREBUILD.md`.
- **Legal memos (AI research, not legal advice):** summarized in `docs/legal/` and `docs/for-casey-review.md`.
