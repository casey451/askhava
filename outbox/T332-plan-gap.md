---
id: T332-plan-gap
title: T332: plan gap reported, blocked for a plan fix
kind: blocked
---
T332's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/confirm_and_pay.rs:393-413 (td_fee_25_an_ended_pick_is_never_cleared, TD-FEE-25, T189 DONE; also owned by T287, T337, T352; not T332) asserts paid_release on an ended pick with a verified success stores nothing (evidence empty line ~408, attempt stays "processing" line ~413). T332 goal part (2) and D-2290 require paid_release to store the success and evidence and call late_success, then answer PickEnded. No implementation passes both. D-2290 and D-2970 do not name confirm_and_pay.rs. Smallest plan change: T332 may also change only that test's two "stores nothing" assertions (evidence row exists, attempt succeeded, one payments.late_success_refund task and one payments.late_success alert; Err(PickEnded) stays) and its doc comment; also the late_success.rs:197-200 comment "paid_release stores no success". Keep T352 ordered after T332. Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T332 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T332 --reason "..."` brings it back with the attempts it has left (D-2859).
