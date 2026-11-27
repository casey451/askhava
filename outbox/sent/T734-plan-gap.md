---
id: T734-plan-gap
title: T734: plan gap reported, blocked for a plan fix
kind: blocked
---
T734's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/templates/business_leave.html:25 owned by T228 and T410, not T734. Smallest plan change: let T734 own that template for one line, <link rel="stylesheet" href="/static/portals/forms.css"> at the top of {% block main %}, as get_help.html:9 and notice_settings.html:8 carry.
Evidence: the six reason radios on /b/leave and /b/pause-reason have no visible edge without forms.css (FORMS_CSS, app/portals/src/feedback.rs:73), so the A11Y-007 edge check (tests/a11y/targets_contrast.spec.ts:80) fails at 320, 768 and 1280 px. The reviewer confirmed from kit/tasks.json that T734 does not own the template, and no file T734 owns can fix it cleanly.
The J1 step 6 e2e failure looks unrelated (a flake or the D-5955 clock issue): J1 and business_leave passed together locally.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T734 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T734 --reason "..."` brings it back with the attempts it has left (D-2859).
