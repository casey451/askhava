---
id: T362-plan-gap
title: T362: plan gap reported, blocked for a plan fix
kind: blocked
---
T362's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/a11y/j1_keyboard.spec.ts:171-172 (owned by T098/T214, protected, not in T362's files) presses Tab before the Go back page's load event. Since T362 serves /static/portals/quote_page.js (404 before), page load waits for the script, so Tab starts at control 0 instead of quote #2 (index 3): "Tab from "" ... Expected: 3, Received: 0" at 320/768/1280 px. Evidence: with the built .js the 3 tests fail; with the .js removed they pass; with await page.waitForLoadState("load") added after the heading check at :171 they pass. T362's own tests pass (static_files_route 5 passed, static_served.spec 1 passed). Reviewer confirmed ownership and cause. Fix: a harness task adds that one line.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T362 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T362 --reason "..."` brings it back with the attempts it has left (D-2859).
