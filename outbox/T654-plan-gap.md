---
id: T654-plan-gap
title: T654: plan gap reported, blocked for a plan fix
kind: blocked
---
T654's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/quotes/src/service.rs, app/quotes/src/late_cards.rs, app/intake/src/cards_out.rs, app/intake/src/order_dispatch.rs, tests/integration/tests/card_pool_queries.rs, tests/integration/tests/radius_matching.rs
Evidence: part (3) needs `now` (the caller's worker clock) in card_pool/one_business (app/quotes/src/card_pool.rs:74,227), which take no clock. Adding one changes every card_pool caller (order_dispatch.rs:831,1007; cards_out.rs:358; late_cards.rs:160; quotes/service.rs:204; card_pool_queries.rs:109; radius_matching.rs:198,210), none owned by T654, while T654.md:11 requires existing card-pool tests pass unchanged. Wall clock is forbidden (T654.md:9). Reviewer confirmed. Smallest fix: give T654 those callers/tests, or split a prior task adding a now parameter.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T654 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T654 --reason "..."` brings it back with the attempts it has left (D-2859).
