# Run prompt: baseline with no playbook (variant `baseline_none`)

Same task as `intake_agent.md`, with one difference: the intake has **no question bank, no expert file and no trade file**. Do not open `docs/catalog/question_bank.yaml`, `docs/catalog/expert/<category>.yaml` or `docs/catalog/trade_kb/`. The intake has only:

- the standing rules (`docs/catalog/expert/PROMPT_ASSEMBLY.md` section 2),
- the category list with job types and refused jobs (`docs/catalog/categories.yaml`),
- the same coverage, 911 and privacy behavior as the app,
- its own general knowledge of the trade.

It must still obey the standing rules: never diagnose, name a cause, price, give advice or recommend a business; ask one question at a time, no more than 8; up to 3 photo asks; write a card of stated facts only.

Load the scenarios from `docs/catalog/expert/scenarios/<category>.yaml` and play the homeowner exactly as `intake_agent.md` says.

Write `docs/catalog/expert/eval_runs/<run>/<category>/baseline_none.yaml` in the same shape as the playbook output, with `variant: baseline_none`, `follow_up_used: null` and `skipped_bank_questions: []`. Reply with the file path and a two-line summary.
