---
id: T671-plan-gap
title: T671: plan gap reported, blocked for a plan fix
kind: blocked
---
T671's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: three tests outside T671's scope fail because of the new behaviour (worker edited none of them):
1. tests/integration/tests/inbound_text_guards.rs:306-308 (T317): quoted_job helper expects begin_job to return Opened::Created after a first text went through the route; the first text now opens the request's job early as D-4360 requires, so begin_job returns Opened::Existing (INT-009). Smallest fix: helper accepts Existing too.
2. tests/integration/tests/inbound_scrub_context.rs:298 (T257/T317): table scan finds "zebulon" in quotes.job_description, from OTHER homeowner's trade-naming first text "my zebulon brand heater", whose scrubbed words are kept per PRIV-008 (not a leak). Smallest fix: check only her rows in that table.
3. app/quotes/src/update_texts.rs NOT_UPDATE_TEXTS ~:145 (T341/T103/T293/T213/T278/T318): td_txt_25_every_homeowner_template_is_scanned_or_left_out fails "two_jobs: scanned false, left out false"; the prompt requires the two_jobs template. Smallest fix: add "two_jobs" beside "split_offer".
Evidence: cargo test --workspace --no-fail-fast exit 101 with exactly these three failures; smoke exit 101 at inbound_scrub_context. Reviewer (opus) re-ran them and CONFIRMED ownership in kit/tasks.json and that the spec requires the behaviour.
Work committed at 290f819ef on claude/task/T671-intake-order-area-trade.
Reviewer findings for the retry to fix (app/intake/src/order_dispatch.rs):
(a) :334-339 late_trade writes trade_reading before begin_job; begin_job refuses replies of <=2 words (e.g. "plumber") or returns Existing, and :330 then ignores every later reply: request stalls for good.
(b) :135/:312/:330 fit_step trade question never asked (goal (3)); no-trade first text gets nothing after the ZIP; a short trade-naming first text ("need plumber") stor

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T671 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T671 --reason "..."` brings it back with the attempts it has left (D-2859).
