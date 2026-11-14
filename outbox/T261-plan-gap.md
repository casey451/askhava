---
id: T261-plan-gap
title: T261: plan gap reported, blocked for a plan fix
kind: blocked
---
T261's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/picks/src/cant_take.rs:64-71 hard-codes end reason "cant_take" (models::end_pick is pub(crate), models.rs:95); migrations/0158_picks_share_box.sql:28-29 CHECK allows only replaced/paused/ineligible/cant_take. T261 goal (1) and D-1871 (3) need the pick to end through the Can't take it path with its own reason, but T261 owns neither app/picks nor migrations, and no other task in kit/tasks.json builds a new reason (only T287 and T261 mention it). Reviewer confirmed with file:line evidence. Secondary: the task needs about 11 files and over 600 lines against a 7-file, 600-line budget. Nothing was committed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T261 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T261 --reason "..."` brings it back with the attempts it has left (D-2859).
