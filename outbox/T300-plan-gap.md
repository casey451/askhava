---
id: T300-plan-gap
title: T300: plan gap reported, blocked for a plan fix
kind: blocked
---
T300's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/templates/business_home.html:27 and app/portals/templates/notice_settings.html:20 (T085/T199 and T196 own them; T300 owns no templates or static files, kit/prompts/T300.md:160) draw native checkboxes and a radio with no styling; Chromium reports a 0px border and transparent fill, so tests/a11y/targets_contrast.spec.ts:80 (A11Y-007) fails on the new /b and /b/notices page states (same cause as D-1820 for the feedback page). Reviewer confirmed by reading. Smallest fix: a portals task before T300 giving those inputs a visible edge (like feedback.css) or one shared rule in app/site/templates/base.html. Router half is built and passes (business_portal_route 4 passed, clippy and fmt clean) but is uncommitted; a copy is in /tmp/T300-router-half.patch.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T300 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T300 --reason "..."` brings it back with the attempts it has left (D-2859).
