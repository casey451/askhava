---
id: T401-plan-gap
title: T401: plan gap reported, blocked for a plan fix
kind: blocked
---
T401's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/security/tests/no_homeowner_charge.rs:17-27,123 (TD-FEE-07, not T401's) fails any app/ file naming a homeowner mark (door_matrix.rs already names Realm::Homeowner at ~969/979) plus askhava_payments/payments::api; T401 needs payments::api::no_fee_release in door_matrix.rs (kit/prompts/T401.md:9). Evidence: cargo test -p askhava-security --test no_homeowner_charge -> td_fee_07_no_homeowner_route_reaches_payments FAILED: door_matrix.rs: homeowner-realm code reaches payments. Reviewer confirmed; no legitimate workaround. Smallest plan change: give T401 a second owned file (e.g. app/server/tests/door_matrix/released_pick.rs) holding only the release seeding, size budget 2 files. Patch saved at /tmp/T401-plan-gap.patch.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T401 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T401 --reason "..."` brings it back with the attempts it has left (D-2859).
