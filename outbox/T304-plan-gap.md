---
id: T304-plan-gap
title: T304: plan gap reported, blocked for a plan fix
kind: blocked
---
T304's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/templates/owner_audit_log.html (owned by T205) and app/portals/templates/health.html (owned by T476) fail Ally once served: bare <table> at owner_audit_log.html:8, health.html:19 and :28 with no role=region tabindex=0 aria-label wrapper (as app/site/templates/business.html:43); reflow_spacing.spec.ts A11Y-008/009 fail at 320px, plus color-contrast incomplete on audit log td:nth-child(5). T304 Scope (kit/prompts/T304.md:35-51) does not allow editing them. Reviewer confirmed ownership via kit/tasks.json and the missing wrappers.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T304 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T304 --reason "..."` brings it back with the attempts it has left (D-2859).
