---
id: T119-plan-gap
title: T119: plan gap reported, blocked for a plan fix
kind: blocked
---
T119's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: tests/e2e/webserver.ts
keyedEnv (webserver.ts:143-177) sets no ASKHAVA_HOMEOWNER_NUMBER; homeowner_number (app/server/src/lib.rs:2345) returns None in CI, so homeowner outbox rows stay queued and J4 steps 1-2 cannot run By text (D-5657). appEnv drops inherited ASKHAVA_* vars, so no workaround. webserver.ts is owned by T536 and not owned/shared by T119. Smallest fix: add a fake +1 ASKHAVA_HOMEOWNER_NUMBER to keyedEnv (or make webserver.ts shared for T119).
Also: stand-in twilio fail fault answers 500 -> OutcomeUnknown, no retries (service.rs refused()); docs/test-design.md J4 step 1 expects 3 retries. T119 can use the 4xx suspended fault (21606) instead; log in decisions.
Reviewer acb751b34eb830299 confirmed: PLAN-GAP: CONFIRMED.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T119 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T119 --reason "..."` brings it back with the attempts it has left (D-2859).
