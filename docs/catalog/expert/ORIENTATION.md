# Expert layer: orientation and gap analysis

Written October 29, 2026 by a Claude session Casey asked to "make the AIs that speak to customers and businesses more knowledgeable than the best people in the fields they represent." This file says what the repo already had, where it fell short of that standard, and what the expert layer adds. Nothing in `sources/`, `BUILD_BRIEF.md`, `docs/spec/` or `docs/constitution.md` was edited; proposals are in `PROPOSED-RULE-CHANGES.md` and open choices in `DECISIONS-NEEDED.md`.

## 1. What the plan already does

The intake AI (Claude Haiku, AI-001) has no tools and never sees the web. Everything it "knows" is handed to it in the prompt, per request, from the catalog:

| Piece | File | What it holds | Where the spec uses it |
|---|---|---|---|
| Category catalog | `docs/catalog/categories.yaml` | 39 categories (37 on), each with its license rule, job types, quote format, recurring flag, refused jobs | LIC-001, LIC-046, PICK-053 |
| License map | `docs/catalog/license_map.yaml` | 59 ROC classes and agencies mapped to job types; routing reads only this | LIC-043, CARD-001 |
| Question bank | `docs/catalog/question_bank.yaml` | One set per category, 3 to 6 fixed questions (up to 8 allowed) in price-impact order, English and Spanish draft, up to 3 photo asks; `kb_trade` names its expert brief | INT-039, LIC-040, INT-044 |
| Trade knowledge base (Grok, D-431) | `docs/catalog/trade_kb/trades/*.yaml` (37 files) plus `foundation/` | Per trade: job types (309 in all), trigger phrases (English and Spanish), a longer ordered question list with "why", photo asks and photo cues, license classes per job, handyman rule, local notes, a sample card, 10 test messages | The "expert brief" the AI may draw one follow-up from (INT-039, AI-019) |
| Worked fee examples | `docs/catalog/worked_examples.yaml` | 40 quote-to-fee rows | FEE-034 |
| Golden intake set | `docs/catalog/intake_golden.yaml` (not yet written; task T154) | About 20 fake requests per launch-open trade with expected trade, job type and questions | INT-040, OPS-040 |

What the AI may do with it (AI-019, D-430): reword the next bank question to fit what the homeowner said, skip one she already answered, ask **one** follow-up the expert brief allows, write the job card and project file from her scrubbed words, answers and photo notes. It never diagnoses, names a cause, prices, judges safety or urgency, or recommends a business; code checks every output for cause words, dollar amounts and contact details.

So the expert standard has to be met inside those limits. The expertise shows up as: the right job type picked from vague words, the few questions that decide the price, the one follow-up that separates the look-alike jobs, the right photos, and a card a business can quote from without calling.

## 2. Where it fell short of "better than the best estimator"

Checked by reading every catalog file and running `scripts/check_catalog.py` (0 errors) and a coverage script.

1. **No estimator's reasoning per job type.** The trade files list questions and "why" lines, but nothing says, for a given job type, which few facts move the price most, which hidden conditions blow up a quote, or which look-alike job the homeowner's words could really be ("toilet gurgles" is a main-line backup, not a toilet repair). The one follow-up the AI is allowed has no list to pick from; it would have to invent one from a 10 to 40 KB file every time.
2. **Eight categories have no expert brief at all** (`kb_trade: null`): dog waste pickup, trash-can cleaning, car detailing, pressure washing, dryer-vent cleaning, holiday lights, home watch, insulation. Their question sets are 3 questions written from general practice.
3. **Thirteen trade files are wired to no category**: cabinets and countertops, carpet cleaning, drywall, evaporative coolers, garage doors (off, D-404), gas lines, generators and EV chargers, golf carts, locksmith, RV services, solar, tree and palm trimming, water heaters. Some of this work is a job type inside another category (swamp coolers under HVAC repair, drywall patch under handyman); some is simply not offered (tree and palm trimming, water-heater replacement, locksmith, carpet cleaning), which sits oddly with Casey's "we need to run all service businesses" (D-459). See `DECISIONS-NEEDED.md` item 1.
4. **Category job types are coarse** (plumbing repair: 4 job types; the trade file: 13). Routing and fees work at the coarse level, which is fine; but the questions and the card need the fine level. Nothing mapped one to the other.
5. **No card template per job type.** The AI writes the card from "her words, answers and photo notes" with no model of what a plumber wants on the first line. Grok's `sample_card` is one example per trade.
6. **No business-side knowledge.** The sign-up flow and the recruiting posts have no per-trade answers to "why would I pay $25 for this", no idea what a new customer costs each trade today, and no list of the words each trade uses. Sign-up objections were to be gathered "with 2 or 3 businesses per trade in recruiting" (question bank header), which the hand-run test can now inform.
7. **Testing is one-shot.** The 370 Grok test messages check trade, job type and first question for a single message. Nothing tests a whole intake conversation, a wrong self-diagnosis, a misleading photo, a two-trade job or whether the finished card is quotable without a phone call, and nothing compares the plan's intake against a strong model with no playbook or against a good human estimator.
8. **No improvement loop.** Nothing turns "a business asked a question before quoting" or "quotes came in wildly apart" into a logged change to a question set.
9. **Nineteen open questions** in the trade KB README (license classes read from mirror sites, dock rules, pest categories from a third-party page) are still open.

## 3. What the expert layer adds (this folder)

| File | What it is |
|---|---|
| `schema.yaml` | The shape of a category expert file, with every field explained |
| `<category>.yaml` (37) | One expert file per enabled category: ranked price drivers, hidden conditions, misdiagnosis checks, the allowed follow-ups per job type (the AI picks at most one), must-know facts mapped to bank questions, photo priority, urgency triggers, a card template, never-say lines, local factors, seasonality, and the business side, each with sources |
| `../trade_kb/trades/` + 8 new files | Schema v2 trade briefs for the eight categories that had none, marked as written by Claude, not Grok; `question_bank.yaml` now names them in `kb_trade` |
| `business_signup_knowledge.yaml` | Per trade: how they get customers now, what a new customer costs (with sources), what they dislike about lead services, objections and the plain answer, their words, busy and slow seasons |
| `PROMPT_ASSEMBLY.md` | Exactly what goes into the intake AI's prompt, per request, from these files, with the size budget and the AI-019 output checks, so T024, T156 and T163 can build it |
| `scenarios/<category>.yaml` (37) | 12 or more adversarial conversation scenarios per category, written by agents that did not write the expert files |
| `judges/` | The intake-agent, baseline and judge prompts, so the test can be re-run |
| `eval_runs/<run>/` | Transcripts, cards and scores of each run (fake data only) |
| `TEST-RESULTS.md` | Scores per category against the pass bar, and the baseline comparison |
| `improvement_loop.md` | How live signals become logged question-set and expert-file changes |
| `README.md` | How this folder is organized and maintained by a future AI session |
| `DECISIONS-NEEDED.md`, `PROPOSED-RULE-CHANGES.md`, `UNVERIFIED.md`, `PROGRESS.md` | Choices for Casey, rule proposals, unverified facts, resume point |
| `../../../scripts/check_expert.py`, `tests/test_check_expert.py` | Fails the build when an expert file breaks the schema, names an unknown job type, question or photo, uses a banned word, or carries a cause, price or safety line in a customer-facing string |

## 4. Rules this layer keeps (not negotiable here)

- The AI never diagnoses, prices or gives safety advice; so misdiagnosis checks become **questions**, never card notes like "possible slab leak" (AI-019 code checks block cause words). The card carries what the homeowner said and what a photo shows, in neutral words.
- Only the fire-truck list gets the 911 text (D-436). Expert files list those triggers so the test can check them; they add no other safety wording.
- Routing reads only `license_map.yaml`; an expert file's license notes are for wording and the test, never for who gets a card.
- Banned words (WORD-003, `trade_kb/foundation/schema.yaml` header) and the AI-smell list (`docs/ai_smell.yaml`) apply to every customer-facing string here.
- Fake data only. No real homeowner, business or phone number anywhere in this folder.
