---
id: T849-plan-gap
title: T849: plan gap reported, blocked for a plan fix
kind: blocked
---
T849's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: tests/e2e/web_chat.spec.ts, tests/a11y/page_states.ts
D-5836 puts inbound SMS and action links on the test-control clock. web_chat.spec.ts:64-68 (consentsSince with no contact, DB real clock_timestamp) and page_states.ts:249-250 (action link expires_at = now() + 2 hours in DB time) are not owned by T849. After j1_homeowner.spec.ts:219 advances the clock up to a day, 4 web_chat tests (:111,:131,:183,:206) and 3 site_speed.spec.ts:256 tests (404) fail. Reviewer confirmed. Fix: T849 owns web_chat.spec.ts and filters consentsSince by contact, and page_states.ts seeds link expiry from the control clock (or action links keep SystemClock).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T849 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T849 --reason "..."` brings it back with the attempts it has left (D-2859).
