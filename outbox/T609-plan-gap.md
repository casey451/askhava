---
id: T609-plan-gap
title: T609: plan gap reported, blocked for a plan fix
kind: blocked
dispatched: 2026-11-28
---
T609's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/e2e/web_chat.spec.ts:123 (owned by T489) uses strict locator main.locator('[lang="es"]'); the footer's Espanol link, which D-4252 (a), D-5386 and DESIGN_SPEC:117 require to carry lang="es" hreflang="es", sits inside main (D-3994), so on /request the locator matches two elements (Spanish consent draft and footer link) and the test fails. T609 owns neither web_chat.spec.ts nor base.html (the only way to move the footer out of main), and its acceptance says web_chat.spec.ts passes unchanged. Reviewer confirmed. Smallest plan change: give T609 ownership of tests/e2e/web_chat.spec.ts and scope line 123 to e.g. main.locator('[lang="es"]:not(footer *)'). Also for the next attempt: leave the aria-hidden arrow off the "Send" button on request.html (axe color-contrast incomplete on /request at 320, 768, 1280) and log it in docs/for-casey-review.md.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T609 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T609 --reason "..."` brings it back with the attempts it has left (D-2859).
