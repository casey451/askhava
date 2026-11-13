---
id: T438-plan-gap
title: T438: plan gap reported, blocked for a plan fix
kind: blocked
---
T438's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/test_failure_drills.py:1046 (test_usage_feeds_the_pacing_windows; owned by no task, T438 may not change it) compares a whole new ledger window to {"start": ..., "tokens": 2000}, but kit/prompts/T438.md requires a window that starts or rolls over to start marked (counting: "message_id"), with kit/pacing.json pre_dedupe_factor 0.6. The test fails only on the extra key {'counting': 'message_id'}. The prompt says stop and report. Fix: let T438 (or a harness task) change the expected value to {"start": ..., "tokens": 2000, "counting": "message_id"}, logged in docs/decisions.md under BUILD_BRIEF.md:69. Confirmed by the reviewer subagent (ran the test; ownership in kit/tasks.json). Work is uncommitted in the worker tree; nothing was run green on smoke or prehandin.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T438 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T438 --reason "..."` brings it back with the attempts it has left (D-2859).
