# Progress: expert layer build

Session of October 28 to 29, 2026 (Casey asked for "the AIs that speak to customers and businesses more knowledgeable than the best people in the fields they represent", then "I need that aspect of the project completed", scope: everything, all trades, delivered as a zip). Branch `claude/expert-layer`, never pushed; delivered as a zip and a git bundle.

A fresh session resumes from this file. Run `python3 scripts/check_expert.py` first; its output lists anything missing.

## Done
- ORIENTATION.md (gap analysis), schema.yaml, PROMPT_ASSEMBLY.md, improvement_loop.md, README.md
- scripts/check_expert.py, tests/test_check_expert.py, scripts/expert_eval_report.py, scripts/build_business_signup_knowledge.py
- judges/ (README, intake_agent, baseline_no_playbook, baseline_human_estimator, playbook_v2, playbook_v3, blind_pro_panel, compliance_judge)
- Expert files for all 37 enabled categories; checker clean (Errors: 0)
- Trade KB files (schema v2, written by Claude) for the 8 categories that had none: dog_waste_pickup, trash_can_cleaning, car_detailing, pressure_washing, dryer_vent_cleaning, holiday_lights, home_watch, insulation; `kb_trade` and `photos` wired in question_bank.yaml for those 8 sets
- scenarios/<category>.yaml for all 37 categories (565 scenarios)
- business_signup_knowledge.yaml (37 trades)
- eval_runs/run-2026-10-29/: playbook, baseline_none, baseline_human and judgement for 9 categories (plumbing_repair, hvac_repair, electrical, house_cleaning, landscape_maintenance, pool_cleaning, pool_repair, pest_control, roof_repair); playbook_v2 for 4 (plumbing, house_cleaning, electrical, hvac_repair); playbook_v3 for 3 (plumbing, electrical, hvac_repair)
- TEST-RESULTS.md, DECISIONS-NEEDED.md, PROPOSED-RULE-CHANGES.md, UNVERIFIED.md
- docs/handoff/Expert_Knowledge_Library_Report.md
- Wired into the build (D-480): requirement AI-020 in docs/spec/ai.md, test entry TD-AI-09, build task T172 (catalog loads, checks and slices the expert files), T154/T156/T163 read the library and T156/T163 depend on T172

## Not done
- Runs and judgments for the other 28 categories (about 20 sub-agent runs per category at three variants plus a judge; roughly one hour of wall time per batch of six categories)
- Fixing each judged category's expert file from its `where_playbook_lost_to_human` list and re-running (the loop in improvement_loop.md); the lists are in the judgment files, the fixes are not yet applied
- playbook_v3 for the other six judged categories, so the proposal in PROPOSED-RULE-CHANGES.md item 1 rests on three categories
- Spanish review of every `_es` line (TXT-003; DECISIONS 4)
- Casey's decisions (DECISIONS-NEEDED.md) and the catalog changes that follow from them

## Order of work if resuming
1. `python3 scripts/check_expert.py`; fix anything it names.
2. For each category without `eval_runs/<run>/<category>/judgement.yaml`: run `judges/intake_agent.md` (Haiku), `baseline_no_playbook.md` (Haiku), `baseline_human_estimator.md` (Opus), then `blind_pro_panel.md` (Opus, which appends the compliance block). Add `playbook_v3.md` (Haiku) and an addendum judge when testing the proposal.
3. `python3 scripts/expert_eval_report.py run-2026-10-29`.
4. For every category below the bar: apply the judgment's fix list to the expert file, re-run that category only, re-judge, log the change in the file's `changelog`.
5. `python3 scripts/build_business_signup_knowledge.py` after any expert file edit.
6. Update TEST-RESULTS.md commentary and the handoff report.
