# Run prompt: the human bar (variant `baseline_human`)

Same task as `intake_agent.md`, but the intake is not Ask Hava's AI. It is **the best estimator in this trade in Arizona**: 25 years in the field, has priced ten thousand jobs from a text and a photo, knows Lake Havasu homes, water, heat and soil. This is the bar the playbook has to meet or beat.

The estimator:
- asks whatever they would really ask a homeowner by text, in their own words, one question at a time, and stops when they could price it (count every question);
- may think out loud about likely causes in their own head, and may even tell the homeowner what they suspect (a real estimator does); the judge notes it, because Ask Hava's intake may not, but it does not count against the human here;
- asks for the photos a real estimator wants;
- writes the job notes they would hand to their crew: the facts they got, what they suspect, what they would check on site;
- keeps only the privacy rules: no name, phone, email or street number in the notes.

Do not open `docs/catalog/question_bank.yaml`, `docs/catalog/expert/<category>.yaml` or `docs/catalog/trade_kb/`. The estimator has the category list (`docs/catalog/categories.yaml`) so the routing field can be filled, and the same coverage behavior (outside area and no businesses stop the intake).

Load the scenarios from `docs/catalog/expert/scenarios/<category>.yaml` and play the homeowner exactly as `intake_agent.md` says. The homeowner does not become more talkative for a human.

Write `docs/catalog/expert/eval_runs/<run>/<category>/baseline_human.yaml` in the same shape as the playbook output, with `variant: baseline_human`, the estimator's notes in `card`, and `follow_up_used: null`, `skipped_bank_questions: []`. Reply with the file path and a two-line summary.
