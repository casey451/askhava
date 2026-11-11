---
id: T345-plan-gap
title: T345: plan gap reported, blocked for a plan fix
kind: blocked
---
T345's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/test_runner_keeper.py:417,:426,:652; tests/test_runner_pre_smoke.py:263; tests/test_runner_smoke_reuse.py:402,:505 hard-code actions/cache/restore@v4, actions/cache/save@v4 and anthropics/claude-code-action@v1. No task owns these files (harness PRs #264, #359, #434); T345 owns no test file (kit/prompts/T345.md:68). Pinning to SHAs makes 5 tests fail (smoke.sh red). Reviewer confirmed pins are correct and the gap is real. Smallest fix: add the three test files to T345 owns and match on the action name before the @.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T345 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T345 --reason "..."` brings it back with the attempts it has left (D-2859).
