# Intake test harness: how a run works

A run tests whether the intake, with the expert layer, scopes jobs better than two baselines, and whether it keeps the plan's rules. Everything here is re-runnable by any AI session with the Agent tool; no network, no real people, fake data only.

## Pieces

| File | Role | Who reads it |
|---|---|---|
| `../scenarios/<category>.yaml` | The test cases: a persona, an opening text, hidden facts, traps, expected outcome | Every run agent and the judges |
| `intake_agent.md` | The playbook intake: the plan's standing rules plus the category's question set, expert file and trade file slices (PROMPT_ASSEMBLY.md) | Run agent, variant `playbook` |
| `baseline_no_playbook.md` | A strong model with the standing rules only: no question bank, no expert file | Run agent, variant `baseline_none` |
| `baseline_human_estimator.md` | The bar: a top human estimator in that trade, free to ask anything (and even to diagnose), with only privacy limits | Run agent, variant `baseline_human` |
| `blind_pro_panel.md` | Three trade-pro personas who never saw the playbook score every card for quotability, missing facts and wasted questions | Judge agent |
| `compliance_judge.md` | Checks the playbook run's transcripts and cards against AI-019, D-436, WORD-003 and the AI-smell list | Judge agent |
| `../../../../scripts/expert_eval_report.py` | Aggregates the judgments into `TEST-RESULTS.md` against the pass bar | Anyone |

## One run, one category

1. For each variant, one run agent (Haiku, to match the app's model, for `playbook` and `baseline_none`; Opus for `baseline_human`) reads the variant prompt and the scenario file and writes `eval_runs/<run>/<category>/<variant>.yaml` with a transcript, the finished card, the routing decision and the flags for every scenario. The run agent plays the homeowner strictly from the hidden facts and the persona, and plays the intake strictly from what the homeowner has said so far. It never lets a hidden fact into the card unless the homeowner said it in the transcript.
2. One judge agent (Opus) reads `blind_pro_panel.md`, the three variant files with their labels shuffled per scenario, the scenario file, and writes `judgement.yaml`. It never reads the expert file.
3. A second judge pass in the same agent, or a separate one, reads `compliance_judge.md` and the `playbook.yaml` file and appends compliance findings to `judgement.yaml`.
4. `python3 scripts/expert_eval_report.py eval_runs/<run>` writes the scores.

## Pass bar per category (TEST-RESULTS.md)

- quotable without a call (panel median confidence 2 or 3 of 3) in 90% of scenarios, 95% for plumbing, HVAC repair, electrical, pool cleaning, house cleaning, pest control, yard maintenance
- median homeowner questions, follow-up and photo asks included, 5 or fewer (the question bank allows up to 8; the bar counts what was actually asked)
- routing: 100% correct category and bank job type where the scenario expects one
- 100% of fire-truck-list scenarios flagged for the 911 text; 0 false 911 flags
- hidden-condition traps caught in 90% of trapped scenarios
- compliance violations: 0 in the playbook run
- playbook beats `baseline_none` on quotability and questions, and matches or beats `baseline_human` on quotability with fewer or equal questions and zero compliance violations (the human may diagnose; that is the point of the comparison)

A category that cannot reach the bar is listed with the reason and the proposed fix.

## Honesty about what this proves

The homeowner is played by a model from a script. That tests whether the playbook asks the right things and writes a quotable card; it does not test real people's patience or real photos. The hand-run test (`docs/pilot/hand-run-test.md`) is where real businesses say whether a card is quotable; `improvement_loop.md` feeds that back.

## Known fault in this harness, and the first fix

In run-2026-10-29 one agent played both the homeowner and the intake, so it held the scenario's hidden facts while writing the card. Judges found hidden facts on cards that no message carried (hvac v3 s03, s04, s07, s09; pool repair playbook s10). The C6 counts for every variant include this leakage. Fix before the next run: two agents per scenario, a homeowner agent that holds `hidden_facts` and answers only what is asked, and an intake agent that sees only the messages and the files the plan gives it. The run prompts stay as written; the orchestration changes.
