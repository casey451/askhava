---
id: T881-plan-gap
title: T881: plan gap reported, blocked for a plan fix
kind: blocked
---
T881's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: kit/state_schema.json
The wip.kind enum (kit/state_schema.json:429-434) is ["review","plan_gap"]. T881's acceptance line (kit/prompts/T881.md:11) needs scripts/check_state.py to pass on wip records of kinds stopped and blocked, so the schema must gain both kinds. T881 owns only runner/cycle.py, runner/wip.py and tests/test_runner_wip.py (T881.md:37-39, :45).
Second conflict: goal change (3) (wip_used no longer stops a later attempt's head being kept) breaks tests/test_runner_wip.py:180 (`"wip" not in entry`), which is not in the prompt's superseded-assertion list (:132, :146-150, :164-172, :97-103). Smallest plan fix: add that line to the list, with a docs/decisions.md entry.
Proof: pytest tests/test_runner_wip.py::test_check_state_reads_a_wip_record_of_each_kind fails with "state.tasks[0].wip.kind: 'stopped' is not one of ['review', 'plan_gap']". Reviewer a757becb6ba2e86e5 confirmed both points at HEAD 56386cb11f4378ad8145ddcb806ef599f6f2b8c6.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T881 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T881 --reason "..."` brings it back with the attempts it has left (D-2859).
