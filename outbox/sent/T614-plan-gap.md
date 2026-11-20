---
id: T614-plan-gap
title: T614: plan gap reported, blocked for a plan fix
kind: blocked
---
T614's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/a11y/reflow_spacing.spec.ts:49-54 (T053, protected) flags every TARGETS control with no box as "is hidden", but DESIGN_SPEC docs/design/DESIGN_SPEC.md:102 requires .site-nav hidden below 1024px and Menu hidden from 1024px, so the specified header fails at every width. Also: closed <details> Menu links keep boxes in Chromium (counted as Tab stops by j1_keyboard.spec.ts:56 and j2_keyboard.spec.ts:26) and the open Menu scrolls sideways at 320px (scrollWidth 375); both fixable only in app/site/static/site.css, not owned by T614 (CSP forbids inline style). Smallest plan change: a task owning site.css adds `.menu:not([open]) .menu-list { display: none }` and fits the open list at 320px; T053's spacingProblems skips controls with getClientRects().length === 0. Reviewer (a5de7ec34ddad778b) confirmed: PLAN-GAP: CONFIRMED. Worker run showed 253 failed / 833 passed in tests/a11y.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T614 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T614 --reason "..."` brings it back with the attempts it has left (D-2859).
