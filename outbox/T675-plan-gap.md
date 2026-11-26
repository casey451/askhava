---
id: T675-plan-gap
title: T675: plan gap reported, blocked for a plan fix
kind: blocked
dispatched: 2026-11-26
---
T675's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/intake_order_card.rs:285, tests/integration/tests/intake_refusals_wired.rs:399, tests/integration/tests/start_resumes_order.rs:321, app/server/tests/name_address_answer.rs:376. Four tests T675 does not own script replies from the trade questions straight to name-and-address; step 9 (photo ask, INT-044) now sits between, so they fail. Reviewer confirmed all four fail at the cited lines. Also the tree changes app/intake/src/api.rs, app/intake/src/lib.rs, app/server/src/lib.rs, which T675 does not own: 10 files against a budget of 7, 14 with the four tests. Nothing committed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T675 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T675 --reason "..."` brings it back with the attempts it has left (D-2859).
