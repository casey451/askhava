---
id: T317-plan-gap
title: T317: plan gap reported, blocked for a plan fix
kind: blocked
---
T317's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/suspension_mode.rs:69-78 (owned by T279) and app/server/tests/voice_confirm_line.rs:95-104 (owned by T311) also build Webhooks by struct literal with names: Vec::new(); T317 owns neither, and goals (3) and (4) (remove names, add Environment) stop both compiling. Reviewer confirmed: all-public fields, no Default or constructor. Smallest fix: add both files to T317's owned files, say six done tests in goal (5) with the same construction-only rule, and raise the file budget from 7 to 9.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T317 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T317 --reason "..."` brings it back with the attempts it has left (D-2859).
