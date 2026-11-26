---
id: T612-plan-gap
title: T612: plan gap reported, blocked for a plan fix
kind: blocked
---
T612's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/site/static/site.css:96-99 (owner T613, not T612): the "DM Sans fallback" face names only local("Arial") and local("Roboto"); the CI image has Liberation Sans only, so with fonts blocked the 320x740 first-screen check in tests/e2e/site_speed.spec.ts fails (Get quotes bottom 771 of 740). Reviewer confirmed: with local("Liberation Sans") added in-browser the button bottom is 695 px. Smallest fix: add local("Liberation Sans") to that face's src with the same size-adjust figures. Reviewer also found: quote-page animation check at site_speed.spec.ts:321-329 reads getAnimations only after 1200 ms and would miss finished no-fill animations (use animationstart recorder); dead condition at forced_colors.spec.ts:23.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T612 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T612 --reason "..."` brings it back with the attempts it has left (D-2859).
