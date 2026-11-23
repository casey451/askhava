---
id: T836-plan-gap
title: T836: plan gap reported, blocked for a plan fix
kind: blocked
---
T836's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/test_runner_five_lanes.py:176 (test_no_workflow_a_hand_in_starts_cancels_another_lanes_run_when_five_hand_in_together, not owned by T836, from D-3043) asserts cancel-in-progress is exactly False for ci.yml, a11y.yml, twin.yml, governance.yml, auto-pr.yml. That contradicts D-5554 (2), which needs cancel-in-progress true for non-main pushes. smoke.sh fails: AssertionError: ci.yml (workflow) cancels in progress (1 failed, 5766 passed). Reviewer confirmed: T836 owns only backup-runner.yml, ci.yml, twin.yml, a11y.yml, tests/test_ci_workflow.py. Smallest fix: add tests/test_runner_five_lanes.py to T836's owned paths, with a decisions.md note; for ci/twin/a11y it should accept the non-main-push expression, and for governance/auto-pr still require False. Work committed at 5f5d410a3, reviewer approved it.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T836 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T836 --reason "..."` brings it back with the attempts it has left (D-2859).
