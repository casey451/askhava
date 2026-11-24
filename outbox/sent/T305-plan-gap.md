---
id: T305-plan-gap
title: T305: plan gap reported, blocked for a plan fix
kind: blocked
---
T305's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/portals/templates/scorecard.html, app/portals/templates/intake_report.html, app/portals/templates/money_view.html
Owners T158 (scorecard.html) and T434 (intake_report.html, money_view.html); T305 owns none. Ally A11Y-008/009 fails once T305 serves these pages: bare tables not in a labelled focusable region (scorecard.html:17, money_view.html:33, intake_report.html:21; pattern business_billing.html:16) and label/hint/input on one line (intake_report.html:8-11, money_view.html:8-11 and :48-61, scorecard.html:48-52; pattern address_change.html:25-28). Reviewer confirmed; page_states.ts cannot fix it. Smallest plan change: T305 (or a small fix task) also owns these three templates.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T305 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T305 --reason "..."` brings it back with the attempts it has left (D-2859).
