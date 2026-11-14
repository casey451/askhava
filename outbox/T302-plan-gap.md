---
id: T302-plan-gap
title: T302: plan gap reported, blocked for a plan fix
kind: blocked
---
T302's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: T302 depends on T450 (D-3602), whose work is not on main: grep IdentityCode|EmailCode in app/portals/src finds nothing, app/portals/src/tests/signup_code_screens.rs does not exist. T302 owns no portals file, so it cannot serve the email-code and identity-code steps (TD-SEC-45) without T450's Screen::IdentityCode, Screen::EmailCode, route constants and channel parser. kit/state.json on claude/kit-state has no T450 and T302's deps there stop at T415, so the runner started T302 before T450. Reviewer confirmed every claim.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T302 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T302 --reason "..."` brings it back with the attempts it has left (D-2859).
