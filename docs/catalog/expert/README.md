# Expert layer (docs/catalog/expert)

The knowledge that lets Ask Hava's intake AI scope a job like the best estimator in each trade, inside the plan's limits (the AI never diagnoses, prices, judges safety or recommends a business). Written for the next AI session, which knows nothing. Start with `ORIENTATION.md`.

## What is here

```
ORIENTATION.md          what the repo had, the gaps, what this adds
schema.yaml             the shape of a category file, every field explained
PROMPT_ASSEMBLY.md      what the intake AI is given per request, from these files
<category>.yaml         one file per enabled category in docs/catalog/categories.yaml (37)
business_signup_knowledge.yaml   the business side, rolled up for sign-up help and recruiting
scenarios/<category>.yaml        12 or more adversarial intake scenarios per category
judges/                 run and judge prompts for the test harness (README there): the plan's
                        scripted intake (intake_agent.md), two baselines, two rule variants
                        (playbook_v2.md, playbook_v3.md), the blind pro panel and the compliance judge
eval_runs/<run>/        transcripts, cards and judgments of each test run (fake data)
TEST-RESULTS.md         scores per category against the pass bar, with baselines and variants
improvement_loop.md     how live signals become logged changes
DECISIONS-NEEDED.md     choices only Casey can make, with recommendations
PROPOSED-RULE-CHANGES.md  rule and catalog changes this work suggests, not applied
UNVERIFIED.md           facts that could not be confirmed in an official source
PROGRESS.md             resume point for a session that stops mid-way
```

Checks: `python3 scripts/check_expert.py` (schema, references, wording; run by `tests/test_check_expert.py`), `python3 scripts/check_catalog.py` (the catalog it extends), `python3 scripts/expert_eval_report.py <run>` (scores), `python3 scripts/build_business_signup_knowledge.py` (regenerates `business_signup_knowledge.yaml` after any expert file changes).

## What the tests showed, in one paragraph

Nine of the 37 categories went through the harness (the seven high-volume ones plus pool repair and roof repair). The plan's scripted intake (fixed question set, one follow-up, three fixed photos) made a card a blind trade panel could quote without a call in 54% to 93% of scenarios; the same model with no script managed 93% to 100%, and a human estimator 100%. Three rule tweaks to the script (`playbook_v2`) helped electrical and hurt plumbing and house cleaning. Letting the AI choose its questions from the expert file's ranked must-know facts under a cap of eight (`playbook_v3`) is the variant that closed the gap where it was run; `TEST-RESULTS.md` has the table and `PROPOSED-RULE-CHANGES.md` item 1 the proposal. The other 28 categories have expert files and scenarios but no run yet; `PROGRESS.md` says how to run them.

## How the pieces fit

- `docs/catalog/categories.yaml` says which categories exist, their job types and quote formats. Routing and fees work at that level.
- `docs/catalog/license_map.yaml` alone decides which licenses receive a card. Nothing here changes routing.
- `docs/catalog/question_bank.yaml` holds the fixed questions per category and names the trade brief (`kb_trade`) and photo asks.
- `docs/catalog/trade_kb/trades/<trade>.yaml` (Grok, D-431, plus eight written by Claude) holds the trade's job types, triggers, long question list, photos and license notes.
- `docs/catalog/expert/<category>.yaml` sits on top: per job type, the ranked must-know facts, the one follow-up the AI may ask, the photo order, the card template and the never-say lines; per category, the price drivers, hidden conditions, misdiagnosis checks, local factors, seasonality and the business side.

`PROMPT_ASSEMBLY.md` says which slice of each file goes into which AI call.

## How to add or change a job type

1. If the job type is new to the trade, add it to the trade file (`job_types`, triggers, photos, questions) with sources, or set `new_job_type: true` on the expert entry when the trade file is Grok's and should stay as delivered.
2. If routing needs it, it is a catalog change: `categories.yaml` job types and `license_map.yaml` rows, logged in `docs/decisions.md`. The expert file's `bank_job_type` must name a category job type.
3. Add the expert entry: `must_know` (2 to 5), `allowed_follow_ups` (1 to 4; define once, share by id), `photos_priority`, `urgency`, scope lines, `card_template`, `never_say`, `quote_format` matching the category.
4. Add 2 or 3 scenarios to `scenarios/<category>.yaml` for it, written without looking at the entry.
5. Run `scripts/check_expert.py`, then the category through the harness (`judges/README.md`), then `scripts/expert_eval_report.py`.
6. Log the change in `docs/decisions.md` (catalog change) and bump the question set's `version` if a bank question moved.

## How a category file is written (the brief the sub-agents followed)

Read `schema.yaml` and the reference `plumbing_repair.yaml`; read the category's row, question set and trade file; interview five personas in your reasoning (25-year owner, top estimator, field tech, code inspector, owner burned by shared-lead sites) on: what you need to quote without a visit, what homeowners leave out or get wrong, which photo angles matter, what separates a real job from a tire-kicker, what would make you pay for an introduction; write the file; cite every licensing, legal, utility, cost and season fact or mark it `[judgment]`; keep every customer-facing string inside the wording rules; run the checker until it is clean.

## Wording rules in one place

- Plain words, short sentences, grade 6 to 8. "Not sure" is added by the app.
- Never: vetted, pre-screened, leads, lead generation, exclusive, best, recommended, background-checked, trusted, top-rated, #1, referral network, "connect you with", concierge, commission.
- Nothing on `docs/ai_smell.yaml`: no em dashes, no exclamation marks, no brochure words.
- No cause words, no dollar figures and no safety or repair advice to a homeowner. Business answers may state the fee tiers.
- Fake data only.

## Limits this layer never crosses

AI-019 (what the AI may write), D-436 (911 text only), LIC-043 and CARD-001 (license map routes), WORD-003 and WORD-027 (wording), PRIV (no homeowner details anywhere here).
