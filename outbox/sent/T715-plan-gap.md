---
id: T715-plan-gap
title: T715: plan gap reported, blocked for a plan fix
kind: blocked
---
T715's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: `app/server/tests/name_address_answer.rs:262`–`:264` (owned by T432, not this task) expects the name-and-address question (step 11) to be asked on the first reply after the area check, even when step 5 never told the coverage. That is the behaviour D-4441 correction (5) removes, so the test is red with T715's change. Smallest plan change: a harness follow-up (T715's attempt or a new task) that may change that test's `her_request` setup so step 5 finishes before step 11 is expected (e.g. first text through the full reply path / receive_raw_reply, or a reply naming a trade, with coverage businesses), citing D-4441. Nothing loosened.
Evidence: on branch head 328d8366a, `cargo test -p askhava-server --test name_address_answer`: 2 passed, 3 failed (td_int_42_a_regular_answer_lands_on_her_own_contact at :264 "asked after the area check"; td_int_42_a_street_is_no_hazard_and_no_number_asks_again at :310; td_int_42_a_hazard_keeps_its_911_text_and_safe_asks_again at :563). With main's order_dispatch.rs swapped back, 5 passed. `her_request` (:194–:223) calls service::receive_text directly (skipping first_text at replies.rs:201) and answer_zip directly, so no trade_check marker is ever written.
Reviewer confirmed: kit/tasks.json lists name_address_answer.rs under T432 and T673 only; T715 owns order_dispatch.rs, intake_order_corrections.rs, intake_order_area_trade.rs (kit/prompts/T715.md:34–:38); T673's prompt allows only an INT-041 text change to that test (kit/prompts/T673.md:11); the T715 prompt does not mention that test; no implementation of correction (5) (kit/prompts/T715.md:9, :11) can keep it green.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T715 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T715 --reason "..."` brings it back with the attempts it has left (D-2859).
