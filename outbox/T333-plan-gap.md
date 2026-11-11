---
id: T333-plan-gap
title: T333: plan gap reported, blocked for a plan fix
kind: blocked
---
T333's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/payments/src/routes.rs:412 (T259) and app/payments/src/refund_state.rs:34 (T270) / app/picks/src/unreachable_refund.rs:306 (T191, T270). kit/prompts/T333.md:23-26 owns only refunds.rs, refund_stripe.rs, refund_lifecycle.rs, refund_amounts.rs. routes.rs calls refund_succeeded(pool, re, now) without the amount, so a webhook-driven success is booked at the full charge unchecked; picks sends the refunded email when refund_state reads Succeeded, so a mismatched or foreign-currency refund can still send the notice. Reviewer confirmed both lie in files T333 does not own. Rest of T333 committed (3c88e306), smoke and prehandin green.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T333 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T333 --reason "..."` brings it back with the attempts it has left (D-2859).
