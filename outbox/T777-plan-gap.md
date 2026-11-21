---
id: T777-plan-gap
title: T777: plan gap reported, blocked for a plan fix
kind: blocked
---
T777's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/server/tests/business_signin_route.rs
T777's approved_at filter in sign_in_lines fails the T280-owned test d_3562_the_code_goes_to_the_members_own_line_else_own_address (business_signin_route.rs:447), which links a non-owner line (helper `line()` lines 181-192, never approve_phone) and expects SMS. T777 owns only sign_in_lines.rs and sign_in_number_change.rs and requires existing sign-in tests unchanged. Smallest plan change: add that file to T777's owned paths so the helper can have the owner approve the line. Reviewer confirmed (PLAN-GAP: CONFIRMED).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T777 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T777 --reason "..."` brings it back with the attempts it has left (D-2859).
