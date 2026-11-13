---
id: T428-plan-gap
title: T428: plan gap reported, blocked for a plan fix
kind: blocked
---
T428's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: T428 does not own tests/test_check_legal_docs.py:75, tests/integration/tests/fee_at_pick.rs:25, fee_mode_switch.rs:67, fee_fixed_at_pick.rs:334, coverage_email.rs:249, pest_identity.rs:278. With the brand switched to Laftly and AUTHORIZATION_VERSION 2 they fail (old sms-terms version line; TERMS version 1; switch writes 3; terms_version 1 vs 2; hard-coded "Ask Hava: "). T428's line that fee_fixed_at_pick.rs passes unchanged contradicts the version-2 floor. Reviewer confirmed each by reading. Fix: give these files an owner or a harness task before T428.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T428 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T428 --reason "..."` brings it back with the attempts it has left (D-2859).
