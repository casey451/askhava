---
id: T345-plan-gap
title: T345: plan gap reported, blocked for a plan fix
kind: blocked
---
T345's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/test_runner_cache.py:207,:214 and tests/test_runner_merge_early.py:212,:213 hard-code actions/cache/restore@v4, actions/cache/save@v4, anthropics/claude-code-action@v1 and actions/setup-python@v5; no task owns them (added after D-3083 in harness PRs #553 (D-3143) and #552 (D-3141)). Reviewer confirmed: lines exist on main, T345 owns list excludes both files, and with the pins applied the two tests fail (2 failed, 35 passed). Smallest fix: add both files to T345 owns under the D-3083 rule (only action-ref matches change, comparing the name before @); owns and files budget 10 -> 12.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T345 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T345 --reason "..."` brings it back with the attempts it has left (D-2859).
