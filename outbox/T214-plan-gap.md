---
id: T214-plan-gap
title: T214: plan gap reported, blocked for a plan fix
kind: blocked
---
T214's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/page_pick.rs:131 and tests/security/tests/page_pick_privacy.rs:224 (both T246's; page_pick.rs also T343's) and tests/e2e/j1_homeowner.spec.ts:203 (T098's) press the final Pick without the share box, which PICK-049 now requires. Smallest plan change: give T214 those three test files for a fake-value change only (add ("share","yes") to each posted final Pick, and check the box before #final-pick in j1_homeowner), or move the server-side refusal to a harness task that owns them. Evidence: kit/tasks.json ownership (T214 owns none of the three); page_pick.rs:131 posts only quote and :266 expects the code text; page_pick_privacy.rs:224 posts only quote and expects the code text; j1_homeowner.spec.ts:203 clicks #final-pick unticked and expects the Finish your pick heading. Reviewer confirmed: refusing at /pick or /code breaks them, and accepting a missing field breaks PICK-049.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T214 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T214 --reason "..."` brings it back with the attempts it has left (D-2859).
