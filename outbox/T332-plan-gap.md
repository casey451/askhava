---
id: T332-plan-gap
title: T332: plan gap reported, blocked for a plan fix
kind: blocked
---
T332's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/late_success.rs:216 (T064's TD-FEE-12, DONE) clashes with T332's goal and D-2290 (the file changes only in the expected trigger). T332 requires run_charge_check to call late_success in its own round on an ended pick, so the later report call at line 216 answers AlreadyQueued, not Queued; the (1,1) count at line 220 stays. Reviewer confirmed (late.rs:62-102 dedupe key; test lines 202-222). Smallest plan change: T332 may also change that one assertion to Ok(LateSuccess::AlreadyQueued), logged in decisions. Also: D-2290 says migration 0176, the goal says 0177; newest existing is 0174.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T332 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T332 --reason "..."` brings it back with the attempts it has left (D-2859).
