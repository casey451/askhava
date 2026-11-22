---
id: T118-plan-gap
title: T118: plan gap reported, blocked for a plan fix
kind: blocked
---
T118's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/site/templates/request.html, app/site/static/site.css
At 1280x900 the /request chat start screen textarea box spans y=784..914, partly below the viewport; Chrome does not scroll a partly visible control into view, so tests/a11y/j3_keyboard.spec.ts:56 fails with "chat-words: outside the viewport" (via tests/e2e/j3.ts:303, step 5) under A11Y-005 (docs/spec/accessibility.md:9). Reproduced by the reviewer at cb4a620. T118 owns only j3_operator.spec.ts, j3_keyboard.spec.ts, j3.ts. Fix: a site task (T593/T609 area) lays out the start screen so the textarea is never partly cut off at 1280x900, or a logged spec decision on A11Y-005 wording. j3_operator.spec.ts passed (worker run) with the D-5486 session-ageing fix at cb4a620; skipped smoke/prehandin because of the gap.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T118 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T118 --reason "..."` brings it back with the attempts it has left (D-2859).
