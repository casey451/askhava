---
id: T302-plan-gap
title: T302: plan gap reported, blocked for a plan fix
kind: blocked
---
T302's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: T302 cannot be built as scoped. (1) No route constant exists for the email-check and sample-pick screens (app/portals/src/email_check.rs has none); T302 owns no portals file, and D-3641 limits the open sign-up door to the nine existing constants. (2) The goal serves the sample pick in sign-up, but BIZ-042 (docs/spec/businesses.md:46) and D-462 offer it at the first real job card. (3) Budget {files 7, lines 780} is far too small (T280's smaller scope took 1266 lines in 4 attempts); split the task: harness matrices, form and identity chain, then the rest. Reviewer confirmed claims 2 and 3; claim 1 is unrealistic but not enforced.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T302 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T302 --reason "..."` brings it back with the attempts it has left (D-2859).
