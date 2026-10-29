# Progress: expert layer build

Session of October 29, 2026 (Casey asked for "the AIs that speak to customers and businesses more knowledgeable than the best people in the fields they represent", then "I need that aspect of the project completed", scope: everything, all trades, delivered as a zip).

A fresh session resumes from this file. Run `python3 scripts/check_expert.py` first; its output lists what is missing.

## Done
- ORIENTATION.md (gap analysis), schema.yaml, PROMPT_ASSEMBLY.md, improvement_loop.md
- scripts/check_expert.py, tests/test_check_expert.py, scripts/expert_eval_report.py
- judges/ (README, intake_agent, baseline_no_playbook, baseline_human_estimator, blind_pro_panel, compliance_judge)
- Reference expert file: plumbing_repair.yaml (passes the checker)

## In progress (see the checker output for the live state)
- Expert files for the other 36 enabled categories (sub-agents, Opus, from scratchpad brief EXPERT_FILE_BRIEF.md: the brief's text is reproduced in README.md section "How a category file is written")
- Trade KB files for the 8 categories with none (schema v2), then their expert files, then `kb_trade` in question_bank.yaml
- scenarios/<category>.yaml for every category (independent writers)
- eval_runs/run-2026-10-29/: playbook, baseline_none, baseline_human and judgement per category
- business_signup_knowledge.yaml (rolled up from each file's business_side)
- TEST-RESULTS.md, README.md, DECISIONS-NEEDED.md, PROPOSED-RULE-CHANGES.md, UNVERIFIED.md
- Final report in docs/handoff/Expert_Knowledge_Library_Report.md; zip

## Order of work if resuming
1. Missing expert files (checker says which).
2. Missing scenarios (checker says which).
3. Runs and judgments for categories without `eval_runs/<run>/<category>/judgement.yaml`.
4. `python3 scripts/expert_eval_report.py run-2026-10-29`.
5. Fix every category below the bar in its expert file; re-run that category.
6. Docs and report.
