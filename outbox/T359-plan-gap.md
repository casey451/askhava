---
id: T359-plan-gap
title: T359: plan gap reported, blocked for a plan fix
kind: blocked
---
T359's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/payment_jobs.rs:585 (T433, TD-FEE-52) asserts the check round writes no ledger row, and app/server/tests/release_recovery.rs:414 (T254, TD-PICK-49/50, no_fee_written) asserts money_view charges == 0. Both contradict TD-FEE-64 (a success by confirm, check round or webhook leaves one charge ledger entry). T359 owns neither file (kit/tasks.json: T433 owns payment_jobs.rs, T254 owns release_recovery.rs). Reviewer confirmed. Smallest fix: a harness change letting T359 adopt both tests, tightened to expect exactly one charge row keyed charge.<pick>. Staged attempt (16 files) saved at /tmp/T359-attempt.patch; reviewer found no money bugs in it.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T359 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T359 --reason "..."` brings it back with the attempts it has left (D-2859).
